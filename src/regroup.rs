//! Join/split/keep recommendations, exact regrouping proposals, explicit agreement
//! and work-preserving workspace transitions. Signals inform usefulness only; every
//! transition still passes the engine's own safety checks.
use crate::analysis::{self, Footprint};
use crate::coordination::{Body, Scope, emit, obligations};
use crate::publication::{self, published};
use crate::{
    Capture, Client, Engine, Layout, Result, VersionedFile, audit, build_tree, connect, fail,
    install, meta, next_revision, paths, read_file, record, retain, revision, set_meta,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Join,
    Split,
    Keep,
}
/// Evidence behind a recommendation. Counts are observations since the last applied
/// transition, never a score.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signals {
    /// Compiler-derived relationships between the agents' registered scopes.
    pub compiler: Vec<String>,
    /// Scopes both agents registered directly.
    pub declared: Vec<String>,
    pub shared_tasks: Vec<String>,
    /// Stale or unreviewed write attempts.
    pub retries: u64,
    /// Reconsideration obligations raised.
    pub reconsiderations: u64,
    pub waits: u64,
    /// Agent explanations attached to their proposals.
    pub reports: Vec<String>,
    /// An agent reported that the current arrangement is not working.
    pub failing: bool,
    /// Missing or degraded evidence. Uncertainty never splits a group.
    pub uncertain: Vec<String>,
}
impl Signals {
    fn friction(&self) -> bool {
        self.retries + self.reconsiderations + self.waits > 0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalState {
    /// Awaiting explicit agreement from every affected agent.
    Open,
    /// Agreed, but a safety check holds the current arrangement; retried at boundaries.
    Blocked { blocker: String },
    /// The new completed revision of every workspace whose contents changed.
    Applied { revisions: BTreeMap<usize, u64> },
    /// An affected agent disagreed; the arrangement is unchanged.
    Declined { agent: usize, reason: String },
    /// Task or group context changed before application; agreement is void.
    Stale { reason: String },
    /// A keep recommendation: recorded evidence, nothing to apply.
    Kept,
}
/// An exact regrouping proposal. Agreement binds to `context`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    /// None when the engine recommends.
    pub proposer: Option<usize>,
    pub action: Action,
    pub agents: BTreeSet<usize>,
    /// Each affected agent's registered task scopes.
    pub scopes: [Vec<Scope>; 2],
    /// Workspace (space) per agent now and after the transition.
    pub from: [usize; 2],
    pub target: [usize; 2],
    /// Live root per target workspace; a split names a root the host may still provision.
    pub roots: BTreeMap<usize, Option<PathBuf>>,
    pub signals: Signals,
    pub reasoning: String,
    /// Fingerprint of the arrangement and every registered scope.
    pub context: String,
    pub agreed: BTreeSet<usize>,
    pub state: ProposalState,
}
/// An agent's own proposal, with its explanation and whether the arrangement is failing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Propose {
    pub action: Action,
    pub text: String,
    pub failing: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Agree,
    Decline { reason: String },
}
/// Observation positions at the last applied transition.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
struct Mark {
    event: u64,
    request: u64,
    wait: u64,
    proposal: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Move {
    space: usize,
    before: Option<Capture>,
    after: Capture,
}
/// A workspace installation recorded before any file changes, for restart recovery.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Shift {
    moves: Vec<Move>,
    installing: bool,
}

pub(crate) fn tables(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS proposals (seq INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE, body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS shifts (id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS waits (id INTEGER PRIMARY KEY AUTOINCREMENT, agent INTEGER NOT NULL, condition TEXT NOT NULL);",
    )?;
    Ok(())
}
fn proposals(db: &Connection) -> Result<Vec<(u64, Proposal)>> {
    let mut stmt = db.prepare("SELECT seq, body FROM proposals ORDER BY seq")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    rows.map(|row| {
        let (seq, body) = row?;
        Ok((seq as u64, serde_json::from_str(&body)?))
    })
    .collect()
}
fn save(db: &Connection, proposal: &Proposal) -> Result<()> {
    db.execute(
        "UPDATE proposals SET body=? WHERE id=?",
        params![serde_json::to_string(proposal)?, proposal.id],
    )?;
    Ok(())
}
fn announce(db: &Connection, proposal: &Proposal) -> Result<u64> {
    emit(db, 0b11, &Body::Regrouping(Box::new(proposal.clone())))
}
fn max(db: &Connection, sql: &str) -> Result<u64> {
    Ok(db.query_row(sql, [], |r| r.get::<_, i64>(0))? as u64)
}
fn mark(db: &Connection) -> Result<Mark> {
    meta(db, "regroup_mark")?.map_or(Ok(Mark::default()), |m| Ok(serde_json::from_str(&m)?))
}
fn scopes(db: &Connection) -> Result<[Vec<Scope>; 2]> {
    let of = |agent| -> Result<Vec<Scope>> {
        Ok(obligations(db, agent)?
            .into_iter()
            .map(|o| o.scope)
            .collect())
    };
    Ok([of(0)?, of(1)?])
}
/// Task and group context: any change voids agreement to an earlier proposal.
fn context(db: &Connection) -> Result<String> {
    use sha2::Digest;
    let bytes = serde_json::to_vec(&(crate::placement(db)?, scopes(db)?))?;
    Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}
/// The latest publication whose operations a workspace already contains.
fn base_of(db: &Connection, workspace: &Capture) -> Result<Capture> {
    let mut stmt = db.prepare(
        "SELECT json_extract(body, '$.Published.revision') FROM events
         WHERE json_extract(body, '$.Published') IS NOT NULL ORDER BY seq DESC",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    for row in rows {
        let candidate = revision(db, row? as u64)?;
        if workspace.included.is_superset(&candidate.included) {
            return Ok(candidate);
        }
    }
    revision(db, 0)
}
/// Paths written by each author's operations.
fn authored(
    db: &Connection,
    operations: impl IntoIterator<Item = u64>,
) -> Result<[BTreeSet<String>; 2]> {
    let mut paths = [BTreeSet::new(), BTreeSet::new()];
    for operation in operations {
        let record = record(db, operation)?;
        paths[record.agent].extend(record.request.output.into_keys());
    }
    Ok(paths)
}
/// A workspace plus the published state: published files the workspace's own unpublished
/// drafts did not touch are taken; drafts the publication also changed are `overlaps`.
pub(crate) fn merge(
    db: &Connection,
    workspace: &Capture,
    published: &Capture,
) -> Result<(Capture, Vec<String>)> {
    let base = base_of(db, workspace)?;
    let [a, b] = authored(
        db,
        workspace.included.difference(&published.included).copied(),
    )?;
    let own: BTreeSet<_> = a.union(&b).collect();
    let mut merged = workspace.clone();
    let mut overlaps = Vec::new();
    for (path, file) in &published.files {
        // Bytes, not occurrences: a restored file is a new occurrence of unchanged content.
        if base.files[path].file == file.file || merged.files[path].file == file.file {
            continue;
        }
        if !own.contains(path) {
            merged.files.insert(path.clone(), file.clone());
        } else if merged.files[path].file != file.file {
            overlaps.push(path.clone());
        }
    }
    merged.included.extend(published.included.iter().copied());
    Ok((merged, overlaps))
}
/// Paths whose bytes differ; a new occurrence of the same bytes is not a content change.
fn differing(before: &Capture, after: &Capture) -> BTreeSet<String> {
    after
        .files
        .iter()
        .filter(|(p, f)| before.files.get(*p).map(|b| &b.file) != Some(&f.file))
        .map(|(p, _)| p.clone())
        .collect()
}

/// Restart: an uncommitted workspace installation is put back to its recorded before
/// state, only after every affected root matches before or after exactly.
pub(crate) fn recover(engine: &Engine, db: &Connection, layout: &Layout) -> Result<()> {
    let mut stmt = db.prepare("SELECT id, body FROM shifts")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, body) in rows {
        let mut shift: Shift = serde_json::from_str(&body)?;
        if !shift.installing {
            continue;
        }
        for change in &shift.moves {
            retain(&engine.state, &change.after)?;
            let root = &layout.roots[change.space];
            let reliable = (|| -> Result<()> {
                let mut observed = Vec::new();
                paths(root, Path::new(""), &mut observed, true)?;
                if observed.iter().any(|p| !change.after.files.contains_key(p)) {
                    return fail("unenrolled source during transition recovery; evidence retained");
                }
                for (path, after) in &change.after.files {
                    let current = read_file(root, path)?;
                    let before = change
                        .before
                        .as_ref()
                        .and_then(|b| b.files[path].file.clone());
                    if current != before && current != after.file {
                        return fail(format!(
                            "ambiguous interrupted transition source: {path}; evidence retained"
                        ));
                    }
                }
                Ok(())
            })();
            if let Err(error) = reliable {
                engine.save_incident(db, &error.to_string())?;
                return Err(error);
            }
        }
        for change in &shift.moves {
            let root = &layout.roots[change.space];
            for path in change.after.files.keys() {
                let before = change
                    .before
                    .as_ref()
                    .and_then(|b| b.files[path].file.clone());
                if read_file(root, path)? != before {
                    install(root, path, &before)?;
                }
            }
        }
        shift.installing = false;
        db.execute(
            "UPDATE shifts SET body=? WHERE id=?",
            params![serde_json::to_string(&shift)?, id],
        )?;
    }
    Ok(())
}

impl Engine {
    /// Host provisioning: an empty live root, outside controller storage and every other
    /// root, that a split may place an agent in. The host exposes a root to an agent only
    /// as `root(client)` names it.
    pub fn add_workspace(&self, root: &Path) -> Result<usize> {
        let root = std::fs::canonicalize(root)?;
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let layout = self.layout()?;
        if root.starts_with(&self.state)
            || self.state.starts_with(&root)
            || layout
                .roots
                .iter()
                .any(|r| r.starts_with(&root) || root.starts_with(r))
        {
            return fail("workspace roots must be separate from storage and each other");
        }
        let lease = crate::writer_lease(&root.join(".falinks-writer.lock"))?;
        let mut observed = Vec::new();
        paths(&root, Path::new(""), &mut observed, true)?;
        if !observed.is_empty() {
            return fail("a new workspace root must be empty");
        }
        let space = layout.roots.len();
        db.execute(
            "INSERT INTO spaces VALUES(?,?,NULL)",
            params![space as i64, root.to_str().ok_or("non-UTF8 workspace")?],
        )?;
        let mut guard = self.layout.write().map_err(|_| "layout lock poisoned")?;
        guard.roots.push(root);
        guard.completed.push(None);
        drop(guard);
        self.leases
            .lock()
            .map_err(|_| "lease lock poisoned")?
            .push(lease);
        drop(db);
        self.advance()?;
        Ok(space)
    }
    /// The live root the host exposes to this client's runtime.
    pub fn root(&self, client: &Client) -> Result<PathBuf> {
        self.authorize(client)?;
        let layout = self.layout()?;
        Ok(layout.roots[layout.placement[client.agent]].clone())
    }
    /// Workspace (space) per agent; equal entries mean the agents are joined.
    pub fn placement(&self) -> Result<[usize; 2]> {
        Ok(self.layout()?.placement)
    }
    pub fn proposal(&self, id: &str) -> Result<Option<Proposal>> {
        let db = connect(&self.state)?;
        db.query_row("SELECT body FROM proposals WHERE id=?", [id], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
        .map(|b| Ok(serde_json::from_str(&b)?))
        .transpose()
    }
    pub fn proposals(&self) -> Result<Vec<Proposal>> {
        Ok(proposals(&connect(&self.state)?)?
            .into_iter()
            .map(|(_, p)| p)
            .collect())
    }
    /// Host or engine reconsideration: initially, at clean boundaries or when evidence may
    /// have changed. An unchanged recommendation returns the earlier proposal.
    pub fn reconsider(&self) -> Result<Proposal> {
        self.recommendation(None, None)
    }
    /// An agent asks the engine to reconsider the arrangement.
    pub fn recommend(&self, client: &Client) -> Result<Proposal> {
        self.authorize(client)?;
        self.recommendation(None, None)
    }
    /// An agent's own join, split or keep proposal with its explanation.
    pub fn propose(&self, client: &Client, propose: Propose) -> Result<Proposal> {
        self.authorize(client)?;
        self.recommendation(Some(client.agent), Some(propose))
    }
    /// Explicit agreement or disagreement, bound to the exact proposal context.
    pub fn respond(
        &self,
        client: &Client,
        id: &str,
        context: &str,
        response: Response,
    ) -> Result<Proposal> {
        self.authorize(client)?;
        let proposal = {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            let mut proposal = self.proposal(id)?.ok_or("unknown proposal")?;
            if !matches!(
                proposal.state,
                ProposalState::Open | ProposalState::Blocked { .. }
            ) {
                return fail("proposal is not open");
            }
            if !proposal.agents.contains(&client.agent) {
                return fail("only an affected agent can respond");
            }
            if context != proposal.context {
                return fail("a response must bind the proposal's exact context");
            }
            let tx = db.unchecked_transaction()?;
            if crate::regroup::context(&tx)? != proposal.context {
                proposal.state = ProposalState::Stale {
                    reason: "task or group context changed; propose again".into(),
                };
            } else {
                match response {
                    Response::Agree => {
                        proposal.agreed.insert(client.agent);
                    }
                    Response::Decline { reason } => {
                        proposal.state = ProposalState::Declined {
                            agent: client.agent,
                            reason,
                        };
                    }
                }
            }
            save(&tx, &proposal)?;
            announce(&tx, &proposal)?;
            tx.commit()?;
            proposal
        };
        self.signal.notify();
        self.advance()?;
        Ok(self.proposal(&proposal.id)?.unwrap_or(proposal))
    }

    fn signals(&self, db: &Connection, layout: &Layout) -> Result<Signals> {
        let mut signals = Signals::default();
        let scopes = scopes(db)?;
        if self
            .analysis
            .read()
            .map_err(|_| "analysis lock poisoned")?
            .is_empty()
        {
            signals
                .uncertain
                .push("compiler analysis not configured".into());
        }
        let mut evidence = Vec::new();
        for space in layout.placement.iter().collect::<BTreeSet<_>>() {
            evidence.extend(self.evidence(&layout.current(*space)?)?);
        }
        for e in &evidence {
            if !e.errors.is_empty() || !e.unknown.is_empty() {
                signals
                    .uncertain
                    .push(format!("degraded evidence for tree {}", e.tree));
            }
        }
        let sides = [evidence.as_slice()];
        let nodes = |s: &Scope| Footprint {
            all: false,
            nodes: s.nodes.clone(),
        };
        for (agent, own) in scopes.iter().enumerate() {
            if own.is_empty() {
                signals
                    .uncertain
                    .push(format!("agent {agent} registered no scope"));
            }
        }
        for a in &scopes[0] {
            for b in &scopes[1] {
                let pair = format!("0:{} 1:{}", a.id, b.id);
                if analysis::related(&nodes(a), &nodes(b)) {
                    signals.declared.push(pair);
                    continue;
                }
                let (left, right) = (
                    analysis::closure(&a.nodes, &sides),
                    analysis::closure(&b.nodes, &sides),
                );
                if left.all || right.all {
                    signals
                        .uncertain
                        .push(format!("unbounded relevance for {pair}"));
                } else if analysis::related(&left, &nodes(b))
                    || analysis::related(&right, &nodes(a))
                {
                    signals.compiler.push(pair);
                }
            }
        }
        let tasks = |s: &[Scope]| s.iter().map(|s| s.task.clone()).collect::<BTreeSet<_>>();
        signals.shared_tasks = tasks(&scopes[0])
            .intersection(&tasks(&scopes[1]))
            .cloned()
            .collect();
        let mark = mark(db)?;
        signals.retries = db.query_row(
            "SELECT COUNT(*) FROM requests WHERE id>? AND (json_extract(body, '$.outcome.Stale') IS NOT NULL OR json_extract(body, '$.outcome.Unreviewed') IS NOT NULL)",
            [mark.request as i64],
            |r| r.get::<_, i64>(0),
        )? as u64;
        signals.reconsiderations = db.query_row(
            "SELECT COUNT(*) FROM events WHERE seq>? AND json_extract(body, '$.Obligation') IS NOT NULL",
            [mark.event as i64],
            |r| r.get::<_, i64>(0),
        )? as u64;
        signals.waits = db.query_row(
            "SELECT COUNT(*) FROM waits WHERE id>?",
            [mark.wait as i64],
            |r| r.get::<_, i64>(0),
        )? as u64;
        for (seq, earlier) in proposals(db)? {
            if seq > mark.proposal
                && let Some(agent) = earlier.proposer
            {
                signals
                    .reports
                    .push(format!("agent {agent}: {}", earlier.reasoning));
                signals.failing |= earlier.signals.failing;
            }
        }
        Ok(signals)
    }

    fn recommendation(
        &self,
        proposer: Option<usize>,
        propose: Option<Propose>,
    ) -> Result<Proposal> {
        let proposal = {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            let layout = self.layout()?;
            let mut signals = self.signals(&db, &layout)?;
            let joined = layout.placement[0] == layout.placement[1];
            let (mut action, mut reasoning) = match (&propose, proposer) {
                (Some(propose), Some(agent)) => {
                    signals
                        .reports
                        .push(format!("agent {agent}: {}", propose.text));
                    signals.failing |= propose.failing;
                    (propose.action, propose.text.clone())
                }
                _ => decide(joined, &signals),
            };
            if (action == Action::Join && joined) || (action == Action::Split && !joined) {
                action = Action::Keep;
                reasoning = format!("already in the requested arrangement; {reasoning}");
            }
            let scopes = scopes(&db)?;
            let history = proposals(&db)?;
            // Reason-to-change rule: no reversal or repeat without new evidence or a failure report.
            let last = history
                .iter()
                .rev()
                .find(|(_, p)| matches!(p.state, ProposalState::Applied { .. }));
            if let Some((_, last)) = last
                && action != Action::Keep
            {
                let new = signals.failing
                    || last.scopes != scopes
                    || last.signals.compiler != signals.compiler
                    || last.signals.declared != signals.declared
                    || last.signals.shared_tasks != signals.shared_tasks
                    || (action == Action::Join && signals.friction());
                if !new {
                    reasoning = format!(
                        "suppressed {action:?}: it would reverse or repeat {} without materially new evidence or a failure report",
                        last.id
                    );
                    action = Action::Keep;
                }
            }
            let target = match action {
                Action::Keep => layout.placement,
                Action::Join => {
                    let into = layout.placement[0].min(layout.placement[1]);
                    [into, into]
                }
                Action::Split => {
                    let stay = layout.placement[0];
                    let free = (0..layout.roots.len())
                        .find(|s| *s != stay && !layout.placement.contains(s))
                        .unwrap_or(layout.roots.len());
                    [stay, free]
                }
            };
            let context = context(&db)?;
            // An unchanged proposal is not repeated.
            if let Some((_, previous)) = history.last()
                && previous.action == action
                && previous.target == target
                && previous.context == context
                && previous.signals.failing == signals.failing
                && material(&previous.signals) == material(&signals)
            {
                return Ok(previous.clone());
            }
            let seq = max(&db, "SELECT COALESCE(MAX(seq), 0) + 1 FROM proposals")?;
            let proposal = Proposal {
                id: format!("regroup:{seq}"),
                proposer,
                action,
                agents: BTreeSet::from([0, 1]),
                scopes,
                from: layout.placement,
                target,
                roots: target
                    .iter()
                    .map(|s| (*s, layout.roots.get(*s).cloned()))
                    .collect(),
                signals,
                reasoning,
                context,
                agreed: BTreeSet::new(),
                state: match action {
                    Action::Keep => ProposalState::Kept,
                    _ => ProposalState::Open,
                },
            };
            let tx = db.unchecked_transaction()?;
            if action != Action::Keep {
                for (_, mut earlier) in history {
                    if matches!(
                        earlier.state,
                        ProposalState::Open | ProposalState::Blocked { .. }
                    ) {
                        earlier.state = ProposalState::Stale {
                            reason: format!("replaced by {}", proposal.id),
                        };
                        save(&tx, &earlier)?;
                        announce(&tx, &earlier)?;
                    }
                }
            }
            tx.execute(
                "INSERT INTO proposals VALUES(?,?,?)",
                params![seq as i64, proposal.id, serde_json::to_string(&proposal)?],
            )?;
            announce(&tx, &proposal)?;
            tx.commit()?;
            proposal
        };
        self.signal.notify();
        Ok(proposal)
    }

    /// Runs at boundaries: voids proposals whose context changed, applies agreed ones whose
    /// blockers cleared, and brings publications into split workspaces.
    pub(crate) fn advance(&self) -> Result<()> {
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let mut changed = false;
        for (_, mut proposal) in proposals(&db)? {
            let agreed = proposal.agreed == proposal.agents;
            match proposal.state {
                ProposalState::Open | ProposalState::Blocked { .. } => {}
                _ => continue,
            }
            if context(&db)? != proposal.context {
                proposal.state = ProposalState::Stale {
                    reason: "task or group context changed; propose again".into(),
                };
                let tx = db.unchecked_transaction()?;
                save(&tx, &proposal)?;
                announce(&tx, &proposal)?;
                tx.commit()?;
                changed = true;
            } else if agreed {
                changed |= self.attempt(&db, &mut proposal)?;
            }
        }
        changed |= self.synchronize(&db)?;
        drop(db);
        if changed {
            self.signal.notify();
        }
        Ok(())
    }

    /// Independent safety checks. A failure names the specific blocker.
    fn plan(
        &self,
        db: &Connection,
        layout: &Layout,
        proposal: &Proposal,
    ) -> Result<std::result::Result<Vec<(usize, Capture)>, String>> {
        if let Some(reason) = meta(db, "halted")? {
            return Ok(Err(format!("engine halted: {reason}")));
        }
        if let Some(run) = self
            .runs()?
            .into_iter()
            .find(|r| r.outcome.is_none() && r.kind == publication::RunKind::Publication)
        {
            return Ok(Err(format!("publication run {} in progress", run.id)));
        }
        let published = revision(db, published(db)?)?;
        for space in proposal.from {
            if !layout
                .current(space)?
                .included
                .is_superset(&published.included)
            {
                return Ok(Err(format!(
                    "workspace {space} has not incorporated publication {}",
                    published.revision
                )));
            }
        }
        // Unpublished drafts per author, and a check that nothing else differs.
        let drafts =
            |workspace: &Capture| -> Result<std::result::Result<[BTreeSet<String>; 2], String>> {
                let own = authored(
                    db,
                    workspace.included.difference(&published.included).copied(),
                )?;
                for path in differing(&published, workspace) {
                    if !own[0].contains(&path) && !own[1].contains(&path) {
                        return Ok(Err(format!("unattributed difference in {path}")));
                    }
                }
                Ok(Ok(own))
            };
        let next = next_revision(db)?;
        let mut moves = Vec::new();
        match proposal.action {
            Action::Keep => return Ok(Err("a keep proposal has nothing to apply".into())),
            Action::Split => {
                let (stay, target) = (proposal.from[0], proposal.target[1]);
                if target >= layout.roots.len() {
                    return Ok(Err(format!("workspace root {target} is not provisioned")));
                }
                if layout.placement.contains(&target) {
                    return Ok(Err(format!("workspace {target} is occupied")));
                }
                let workspace = layout.current(stay)?;
                let [kept, moved] = match drafts(&workspace)? {
                    Ok(own) => own,
                    Err(blocker) => return Ok(Err(blocker)),
                };
                let interleaved: Vec<_> = kept.intersection(&moved).collect();
                if !interleaved.is_empty() {
                    return Ok(Err(format!(
                        "interleaved unpublished drafts in {interleaved:?}"
                    )));
                }
                let leaving: BTreeSet<u64> = workspace
                    .included
                    .difference(&published.included)
                    .copied()
                    .filter(|op| record(db, *op).is_ok_and(|r| r.agent == 1))
                    .collect();
                let mut created = published.clone();
                created.space = target;
                created.revision = next;
                created.included.extend(leaving.iter().copied());
                for path in &moved {
                    created
                        .files
                        .insert(path.clone(), workspace.files[path].clone());
                }
                created.tree = build_tree(&self.state, &created.files, "")?;
                moves.push((target, created));
                if !moved.is_empty() {
                    // Moved, not lost: the drafts continue in the new workspace.
                    let mut remaining = workspace.clone();
                    remaining.revision = next + 1;
                    remaining.included.retain(|op| !leaving.contains(op));
                    for path in &moved {
                        remaining.files.insert(
                            path.clone(),
                            VersionedFile {
                                version: remaining.revision,
                                ..published.files[path].clone()
                            },
                        );
                    }
                    remaining.tree = build_tree(&self.state, &remaining.files, "")?;
                    moves.push((stay, remaining));
                }
            }
            Action::Join => {
                let into = proposal.target[0];
                let from = *proposal
                    .from
                    .iter()
                    .find(|s| **s != into)
                    .ok_or("join needs two workspaces")?;
                let (host, guest) = (layout.current(into)?, layout.current(from)?);
                let (host_drafts, guest_drafts) = match (drafts(&host)?, drafts(&guest)?) {
                    (Ok([a, b]), Ok([c, d])) => (
                        a.union(&b).cloned().collect::<BTreeSet<_>>(),
                        c.union(&d).cloned().collect::<BTreeSet<_>>(),
                    ),
                    (Err(blocker), _) | (_, Err(blocker)) => return Ok(Err(blocker)),
                };
                let overlap: Vec<_> = host_drafts.intersection(&guest_drafts).collect();
                if !overlap.is_empty() {
                    return Ok(Err(format!(
                        "both workspaces hold unpublished drafts of {overlap:?}"
                    )));
                }
                if !guest_drafts.is_empty() {
                    let mut joined = host.clone();
                    joined.revision = next;
                    joined.included.extend(guest.included.iter().copied());
                    for path in &guest_drafts {
                        joined.files.insert(path.clone(), guest.files[path].clone());
                    }
                    joined.tree = build_tree(&self.state, &joined.files, "")?;
                    moves.push((into, joined));
                }
            }
        }
        Ok(Ok(moves))
    }

    /// Applies an agreed proposal or records its blocker. Returns whether anything changed.
    fn attempt(&self, db: &Connection, proposal: &mut Proposal) -> Result<bool> {
        let layout = self.layout()?;
        let moves = match self.plan(db, &layout, proposal)? {
            Ok(moves) => moves,
            Err(blocker) => {
                let blocked = ProposalState::Blocked { blocker };
                if proposal.state == blocked {
                    return Ok(false);
                }
                proposal.state = blocked;
                let tx = db.unchecked_transaction()?;
                save(&tx, proposal)?;
                announce(&tx, proposal)?;
                tx.commit()?;
                return Ok(true);
            }
        };
        // Each agent's view: its current workspace before, its target workspace after.
        let mut views = Vec::new();
        for agent in 0..2 {
            let before = layout.current(layout.placement[agent])?;
            let target = proposal.target[agent];
            let after = match moves.iter().find(|(s, _)| *s == target) {
                Some((_, capture)) => capture.clone(),
                None => layout.current(target)?,
            };
            let paths = differing(&before, &after);
            if !paths.is_empty() {
                views.push((agent, after.revision, self.view(&before, &after, &paths)?));
            }
        }
        let shift = self.install_moves(db, &layout, &moves)?;
        proposal.state = ProposalState::Applied {
            revisions: moves.iter().map(|(s, c)| (*s, c.revision)).collect(),
        };
        let tx = db.unchecked_transaction()?;
        save(&tx, proposal)?;
        let event = announce(&tx, proposal)?;
        let after = self.commit_moves(&tx, &layout, shift, &moves, proposal.target)?;
        for (agent, revision, view) in &views {
            self.renew_view(&tx, *agent, view, *revision, event)?;
        }
        let mark = Mark {
            event,
            request: max(&tx, "SELECT COALESCE(MAX(id), 0) FROM requests")?,
            wait: max(&tx, "SELECT COALESCE(MAX(id), 0) FROM waits")?,
            proposal: max(&tx, "SELECT COALESCE(MAX(seq), 0) FROM proposals")?,
        };
        set_meta(&tx, "regroup_mark", &serde_json::to_string(&mark)?)?;
        tx.commit()?;
        *self.layout.write().map_err(|_| "layout lock poisoned")? = after;
        Ok(true)
    }

    /// Brings each occupied workspace up to the published state when its own drafts allow.
    fn synchronize(&self, db: &Connection) -> Result<bool> {
        let published = revision(db, published(db)?)?;
        let mut changed = false;
        let occupied: BTreeSet<usize> = self.layout()?.placement.into_iter().collect();
        for space in &occupied {
            let layout = self.layout()?;
            let workspace = layout.current(*space)?;
            if workspace.included.is_superset(&published.included) {
                continue;
            }
            let members: Vec<usize> = (0..2).filter(|a| layout.placement[*a] == *space).collect();
            let audience: i64 = members.iter().map(|a| crate::coordination::mine(*a)).sum();
            let (mut merged, overlaps) = merge(db, &workspace, &published)?;
            if !overlaps.is_empty() {
                let key = format!("behind:{space}");
                let note = format!("{}:{overlaps:?}", published.revision);
                if meta(db, &key)?.as_deref() != Some(note.as_str()) {
                    let tx = db.unchecked_transaction()?;
                    set_meta(&tx, &key, &note)?;
                    emit(
                        &tx,
                        audience,
                        &Body::Behind {
                            space: *space,
                            publication: published.revision,
                            overlaps,
                        },
                    )?;
                    tx.commit()?;
                    changed = true;
                }
                continue;
            }
            if meta(db, "halted")?.is_some() {
                continue;
            }
            merged.revision = next_revision(db)?;
            merged.tree = build_tree(&self.state, &merged.files, "")?;
            let moves = vec![(*space, merged.clone())];
            let view = self.view(&workspace, &merged, &differing(&workspace, &merged))?;
            let shift = self.install_moves(db, &layout, &moves)?;
            let tx = db.unchecked_transaction()?;
            let event = emit(
                &tx,
                audience,
                &Body::Incorporated {
                    space: *space,
                    publication: published.revision,
                    revision: merged.revision,
                },
            )?;
            let after = self.commit_moves(&tx, &layout, shift, &moves, layout.placement)?;
            for agent in members {
                self.renew_view(&tx, agent, &view, merged.revision, event)?;
            }
            tx.commit()?;
            *self.layout.write().map_err(|_| "layout lock poisoned")? = after;
            changed = true;
        }
        Ok(changed)
    }

    /// Retains every new revision and records the installation before touching any root,
    /// then installs and verifies. An interruption halts until restart restores `before`.
    fn install_moves(
        &self,
        db: &Connection,
        layout: &Layout,
        moves: &[(usize, Capture)],
    ) -> Result<i64> {
        let shift = Shift {
            moves: moves
                .iter()
                .map(|(space, after)| Move {
                    space: *space,
                    before: layout.completed[*space].clone(),
                    after: after.clone(),
                })
                .collect(),
            installing: true,
        };
        for change in &shift.moves {
            let root = &layout.roots[change.space];
            let verified = match &change.before {
                Some(before) => audit(root, before),
                None => {
                    let mut observed = Vec::new();
                    paths(root, Path::new(""), &mut observed, true).and_then(|()| {
                        match observed.first() {
                            Some(path) => fail(format!("unexplained source write: {path}")),
                            None => Ok(()),
                        }
                    })
                }
            };
            if let Err(error) = verified {
                self.save_incident(db, &error.to_string())?;
                return Err(error);
            }
            retain(&self.state, &change.after)?;
        }
        db.execute(
            "INSERT INTO shifts(body) VALUES(?)",
            [serde_json::to_string(&shift)?],
        )?;
        let id = db.last_insert_rowid();
        let installed = (|| -> Result<()> {
            for change in &shift.moves {
                let root = &layout.roots[change.space];
                for (path, file) in &change.after.files {
                    let before = change.before.as_ref().map(|b| &b.files[path].file);
                    if before != Some(&file.file) {
                        install(root, path, &file.file)?;
                    }
                }
                audit(root, &change.after)?;
            }
            Ok(())
        })();
        if let Err(error) = installed {
            set_meta(
                db,
                "halted",
                &format!("interrupted transition {id}: {error}"),
            )?;
            return Err(error);
        }
        Ok(id)
    }
    /// Inside the caller's transaction: new revisions, pointers, placement, installed shift.
    fn commit_moves(
        &self,
        tx: &Connection,
        layout: &Layout,
        shift: i64,
        moves: &[(usize, Capture)],
        placement: [usize; 2],
    ) -> Result<Layout> {
        let mut after = layout.clone();
        for (space, capture) in moves {
            tx.execute(
                "INSERT INTO revisions VALUES(?,?)",
                params![capture.revision as i64, serde_json::to_string(capture)?],
            )?;
            tx.execute(
                "UPDATE spaces SET completed=? WHERE id=?",
                params![capture.revision as i64, *space as i64],
            )?;
            after.completed[*space] = Some(capture.clone());
        }
        set_meta(tx, "placement", &serde_json::to_string(&placement)?)?;
        after.placement = placement;
        let body: String =
            tx.query_row("SELECT body FROM shifts WHERE id=?", [shift], |r| r.get(0))?;
        let mut recorded: Shift = serde_json::from_str(&body)?;
        recorded.installing = false;
        tx.execute(
            "UPDATE shifts SET body=? WHERE id=?",
            params![serde_json::to_string(&recorded)?, shift],
        )?;
        Ok(after)
    }
}

/// Evidence that changes a recommendation's meaning; counts only matter as presence.
fn material(signals: &Signals) -> (Vec<String>, Vec<String>, Vec<String>, bool, bool, bool) {
    (
        signals.compiler.clone(),
        signals.declared.clone(),
        signals.shared_tasks.clone(),
        signals.friction(),
        signals.failing,
        signals.uncertain.is_empty(),
    )
}

/// Join for useful unfinished collaboration, split for useful independent progress, and
/// prefer fewer groups when uncertain. A compiler edge alone never forces a join.
fn decide(joined: bool, signals: &Signals) -> (Action, String) {
    let collaboration =
        !signals.declared.is_empty() || !signals.shared_tasks.is_empty() || signals.friction();
    let summary = format!(
        "compiler {:?}, declared {:?}, shared tasks {:?}, retries {}, reconsiderations {}, waits {}",
        signals.compiler,
        signals.declared,
        signals.shared_tasks,
        signals.retries,
        signals.reconsiderations,
        signals.waits
    );
    if signals.failing {
        let action = if joined { Action::Split } else { Action::Join };
        return (
            action,
            format!("an agent reports the arrangement is failing; {summary}"),
        );
    }
    if joined {
        if !signals.uncertain.is_empty() {
            return (
                Action::Keep,
                format!(
                    "uncertain evidence {:?}; prefer fewer groups",
                    signals.uncertain
                ),
            );
        }
        if collaboration || !signals.compiler.is_empty() {
            return (Action::Keep, format!("ongoing collaboration; {summary}"));
        }
        return (
            Action::Split,
            format!("registered scopes look independent; separate progress is useful; {summary}"),
        );
    }
    if collaboration {
        return (
            Action::Join,
            format!("collaboration on unfinished work looks useful; {summary}"),
        );
    }
    if !signals.compiler.is_empty() {
        return (
            Action::Keep,
            format!("a compiler relationship alone does not force a join; {summary}"),
        );
    }
    (
        Action::Keep,
        format!("independent progress continues; {summary}"),
    )
}
