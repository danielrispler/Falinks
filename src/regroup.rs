//! Join/split/keep recommendations, exact regrouping proposals, explicit agreement
//! and work-preserving workspace transitions. Signals inform usefulness only; every
//! transition still passes the engine's own safety checks.
use crate::analysis::{self, Evidence, Footprint};
use crate::coordination::{Body, Decision, Kind, Message, Review, Scope, emit, obligations};
use crate::publication::{self, published};
use crate::{
    Capture, Client, Engine, Fault, Halt, Interrupted, Layout, Result, VersionedFile, WRITER_LEASE,
    all, audit, build_tree, connect, ensure_empty, fail, halted, install, meta, meta_value,
    next_revision, one, record, retain, revision, set_halt, set_meta,
};
use rusqlite::{Connection, params};
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
/// Evidence behind a recommendation, read from current collaboration, never a score.
/// Each agent's current work is its unpublished drafts by owning symbol or, before it has
/// edited and before any applied transition, its registered scopes. The recency window,
/// the latest settled review in each direction, bounds `revised` and `retries` only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signals {
    /// Compiler-derived relationships between the agents' current work.
    pub compiler: Vec<String>,
    /// Registered scopes both agents name, or one declared a dependency on the other's.
    /// Reported; a declaration decides only when added or changed after a split.
    pub declared: Vec<String>,
    /// Reported only; task labels decide nothing.
    pub shared_tasks: Vec<String>,
    /// Stale write refusals in the recency window: collaboration evidence.
    pub retries: u64,
    /// Reconsideration obligations raised by agents' live edits since the last applied
    /// transition. Reported only.
    pub reconsiderations: u64,
    /// Reported only.
    pub waits: u64,
    /// Agent explanations attached to their proposals.
    pub reports: Vec<String>,
    /// An agent reported that the current arrangement is not working.
    pub failing: bool,
    /// Missing or degraded evidence, including unreviewed obligations: keep the arrangement.
    pub uncertain: Vec<String>,
    /// Current work of both agents includes the same symbol.
    #[serde(default)]
    pub overlap: Vec<String>,
    /// Latest settled reviews that decided `Revise`: collaboration evidence.
    #[serde(default)]
    pub revised: Vec<String>,
    /// Scopes whose declared dependencies were added or changed since the split.
    #[serde(default)]
    pub redeclared: Vec<String>,
    /// Files a publication overlapped in a split workspace since the split.
    #[serde(default)]
    pub behind: Vec<String>,
    /// Pending offers: checkpoint offers still available at their agent's current revision.
    #[serde(default)]
    pub pending: Vec<String>,
}
impl Signals {
    /// Current collaboration that keeps joined agents together.
    fn collaborating(&self) -> bool {
        !self.overlap.is_empty()
            || !self.compiler.is_empty()
            || !self.revised.is_empty()
            || self.retries > 0
    }
    /// Evidence that joins split agents.
    fn joining(&self) -> bool {
        !self.overlap.is_empty() || !self.redeclared.is_empty() || !self.behind.is_empty()
    }
    /// Join evidence a transition cannot have seen: overlap between drafts or an overlapping
    /// publication since it, never registered scopes (#357).
    fn rejoining(&self) -> bool {
        let drafts = format!("0:{DRAFTS} 1:{DRAFTS}");
        self.overlap.contains(&drafts) || !self.behind.is_empty()
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
    /// Agreement is void: the task or group context changed, a newer proposal replaced it,
    /// or unpublished drafts made it unsafe until a publication. The reason says which.
    Stale { reason: String },
    /// A keep recommendation: recorded evidence, nothing to apply.
    Kept,
}
impl ProposalState {
    /// Still awaiting agreement or application.
    pub fn pending(&self) -> bool {
        matches!(self, Self::Open | Self::Blocked { .. })
    }
}
/// An exact regrouping proposal. Agreement binds to `context`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegroupingProposal {
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
pub(crate) struct RootChange {
    pub(crate) space: usize,
    pub(crate) before: Option<Capture>,
    pub(crate) after: Capture,
}
/// A workspace installation recorded before any file changes, for restart recovery.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Installation {
    pub(crate) moves: Vec<RootChange>,
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
fn proposals(db: &Connection) -> Result<Vec<(u64, RegroupingProposal)>> {
    all(
        db,
        "SELECT json_array(seq, json(body)) FROM proposals ORDER BY seq",
        [],
    )
}
fn proposal(db: &Connection, id: &str) -> Result<Option<RegroupingProposal>> {
    one(db, "SELECT body FROM proposals WHERE id=?", [id])
}
fn save(db: &Connection, proposal: &RegroupingProposal) -> Result<()> {
    db.execute(
        "UPDATE proposals SET body=? WHERE id=?",
        params![serde_json::to_string(proposal)?, proposal.id],
    )?;
    Ok(())
}
fn announce(db: &Connection, proposal: &RegroupingProposal) -> Result<u64> {
    emit(db, 0b11, &Body::Regrouping(Box::new(proposal.clone())))
}
/// Saves and announces a state change in its own transaction.
fn commit_state(db: &Connection, proposal: &RegroupingProposal) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    save(&tx, proposal)?;
    announce(&tx, proposal)?;
    tx.commit()?;
    Ok(())
}
const CONTEXT_CHANGED: &str = "task or group context changed; propose again";
/// A split refused because of unpublished drafts; it voids agreement (#357).
const UNSAFE: &str = "unsafe until publication";
/// The current-work label of an agent's unpublished drafts.
const DRAFTS: &str = "drafts";
pub(crate) fn scalar(db: &Connection, sql: &str) -> Result<u64> {
    Ok(db.query_row(sql, [], |r| r.get::<_, i64>(0))? as u64)
}
fn mark(db: &Connection) -> Result<Mark> {
    Ok(meta_value(db, "regroup_mark")?.unwrap_or_default())
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
fn current_context(db: &Connection) -> Result<String> {
    context(db, &scopes(db)?)
}
fn context(db: &Connection, scopes: &[Vec<Scope>; 2]) -> Result<String> {
    let bytes = serde_json::to_vec(&(crate::placement(db)?, scopes))?;
    Ok(crate::analysis::sha256(bytes))
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
    let [own_a, own_b] = authored(
        db,
        workspace.included.difference(&published.included).copied(),
    )?;
    let own: BTreeSet<_> = own_a.union(&own_b).collect();
    // Only paths a published operation missing from the workspace wrote can change it:
    // a pre-split candidate holds the workspace's own earlier drafts (#331, E4).
    let [missing_a, missing_b] = authored(
        db,
        published.included.difference(&workspace.included).copied(),
    )?;
    let missing: BTreeSet<_> = missing_a.union(&missing_b).collect();
    let mut merged = workspace.clone();
    let mut overlaps = Vec::new();
    for (path, file) in &published.files {
        // Bytes, not occurrences: a restored file is a new occurrence of unchanged content.
        if !missing.contains(path)
            || base.files[path].file == file.file
            || merged.files[path].file == file.file
        {
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
/// Each author's unpublished drafts in `workspace` by owning symbol: the files it wrote
/// (`authored`) that still differ from the publication. `evidence` reads the publication
/// and the workspace, so a draft that deletes a symbol names it.
fn drafted(
    authored: [&BTreeSet<String>; 2],
    published: &Capture,
    workspace: &Capture,
    evidence: [&[Evidence]; 2],
) -> [Footprint; 2] {
    let differ = differing(published, workspace);
    authored.map(|paths| {
        let paths = paths.intersection(&differ).cloned().collect();
        let change = analysis::change(published, workspace, &paths, &evidence);
        let mut footprint = Footprint {
            all: change.all,
            nodes: BTreeSet::new(),
        };
        owners(
            &change.nodes.into_iter().collect::<Vec<_>>(),
            &mut footprint,
        );
        footprint
    })
}
/// A compiler relationship between the two agents' drafts, named by its first pair of
/// owning symbols: a split would strand one draft's use of the other.
fn stranding(drafts: &[Footprint; 2], sides: &[&[Evidence]]) -> Option<String> {
    let [a, b] = drafts;
    let empty = |f: &Footprint| !f.all && f.nodes.is_empty();
    if empty(a) || empty(b) {
        return None;
    }
    if a.all || b.all {
        return Some("unbounded relevance of unpublished drafts".into());
    }
    let one = |node: &String| BTreeSet::from([node.clone()]);
    let node = |n: &String| Footprint {
        all: false,
        nodes: one(n),
    };
    let closures: Vec<_> = b
        .nodes
        .iter()
        .map(|y| (y, analysis::closure(&one(y), sides)))
        .collect();
    for x in &a.nodes {
        let left = analysis::closure(&one(x), sides);
        for (y, right) in &closures {
            if analysis::related(&left, &node(y)) || analysis::related(right, &node(x)) {
                return Some(format!(
                    "unpublished drafts {x} (agent 0) and {y} (agent 1) relate by the compiler"
                ));
            }
        }
    }
    None
}

/// Shifts still installing at restart, with their new revisions retained as evidence.
/// Restart recovery checks and restores their roots (`Engine::recover`).
pub(crate) fn installing(engine: &Engine, db: &Connection) -> Result<Vec<(i64, Installation)>> {
    let rows: Vec<(i64, Installation)> =
        all(db, "SELECT json_array(id, json(body)) FROM shifts", [])?;
    let mut installing = Vec::new();
    for (id, shift) in rows.into_iter().filter(|(_, s)| s.installing) {
        for change in &shift.moves {
            retain(&engine.state, &change.after)?;
        }
        installing.push((id, shift));
    }
    Ok(installing)
}
/// After restart restored a shift's roots: it is no longer installing.
pub(crate) fn restored(db: &Connection, id: i64, mut shift: Installation) -> Result<()> {
    shift.installing = false;
    db.execute(
        "UPDATE shifts SET body=? WHERE id=?",
        params![serde_json::to_string(&shift)?, id],
    )?;
    Ok(())
}

impl Engine {
    /// Host provisioning: an empty live root, outside controller storage and every other
    /// root, that a split may place an agent in. The host exposes a root to an agent only
    /// as `root(client)` names it.
    pub fn add_workspace(&self, root: &Path) -> Result<usize> {
        let root = std::fs::canonicalize(root)?;
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let space = {
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
            layout.roots.len()
        };
        let lease = crate::writer_lease(&root.join(WRITER_LEASE))?;
        ensure_empty(&root).map_err(|_| "a new workspace root must be empty")?;
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
        self.boundary();
        Ok(space)
    }
    /// The live root the host exposes to this client's runtime.
    pub fn root(&self, client: &Client) -> Result<PathBuf> {
        self.authorize(client)?;
        let layout = self.layout()?;
        Ok(layout.roots[layout.of(client.agent)].clone())
    }
    /// Workspace (space) per agent; equal entries mean the agents are joined.
    pub fn placement(&self) -> Result<[usize; 2]> {
        Ok(self.layout()?.placement)
    }
    pub fn proposal(&self, id: &str) -> Result<Option<RegroupingProposal>> {
        proposal(&connect(&self.state)?, id)
    }
    pub fn proposals(&self) -> Result<Vec<RegroupingProposal>> {
        Ok(proposals(&connect(&self.state)?)?
            .into_iter()
            .map(|(_, p)| p)
            .collect())
    }
    /// Host or engine reconsideration: initially, at clean boundaries or when evidence may
    /// have changed. An unchanged recommendation returns the earlier proposal.
    /// The first call also lets the engine reconsider by itself at later boundaries.
    pub fn reconsider(&self) -> Result<RegroupingProposal> {
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        set_meta(&db, "regroup_enabled", "1")?;
        drop(db);
        self.recommendation(None, None)
    }
    /// An agent asks the engine to reconsider the arrangement.
    pub fn recommend(&self, client: &Client) -> Result<RegroupingProposal> {
        self.authorize(client)?;
        self.recommendation(None, None)
    }
    /// An agent's own join, split or keep proposal with its explanation.
    pub fn propose(&self, client: &Client, propose: Propose) -> Result<RegroupingProposal> {
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
    ) -> Result<RegroupingProposal> {
        self.authorize(client)?;
        let proposal = {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            let mut proposal = proposal(&db, id)?.ok_or("unknown proposal")?;
            if !proposal.state.pending() {
                return fail("proposal is not open");
            }
            if !proposal.agents.contains(&client.agent) {
                return fail("only an affected agent can respond");
            }
            if context != proposal.context {
                return fail("a response must bind the proposal's exact context");
            }
            if current_context(&db)? != proposal.context {
                proposal.state = ProposalState::Stale {
                    reason: CONTEXT_CHANGED.into(),
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
            commit_state(&db, &proposal)?;
            proposal
        };
        self.signal.notify();
        self.boundary();
        Ok(self.proposal(&proposal.id)?.unwrap_or(proposal))
    }

    // ponytail: evidence is gathered under the writer lock, like the edit gate; gather
    // outside it if reconsideration delays edits.
    /// The signals, and the compiler relationship between the agents' drafts that a split
    /// would strand.
    fn signals(
        &self,
        db: &Connection,
        layout: &Layout,
        scopes: &[Vec<Scope>; 2],
        history: &[(u64, RegroupingProposal)],
        mark: &Mark,
    ) -> Result<(Signals, Option<String>)> {
        let mut signals = Signals::default();
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
        let published = revision(db, published(db)?)?;
        let before = self.evidence_in(db, &published)?;
        let mut evidence = Vec::new();
        let mut drafts: [Footprint; 2] = Default::default();
        for space in layout.placement.iter().collect::<BTreeSet<_>>() {
            let workspace = layout.current(*space)?;
            let after = self.evidence_in(db, workspace)?;
            let own = authored(
                db,
                workspace.included.difference(&published.included).copied(),
            )?;
            let footprints = drafted(own.each_ref(), &published, workspace, [&before, &after]);
            for (agent, footprint) in footprints.into_iter().enumerate() {
                if layout.placement[agent] == *space {
                    drafts[agent] = footprint;
                }
            }
            evidence.extend(after);
        }
        for e in &evidence {
            if !e.errors.is_empty() || !e.unknown.is_empty() {
                signals
                    .uncertain
                    .push(format!("degraded evidence for tree {}", e.tree));
            }
        }
        let sides = [evidence.as_slice()];
        let nodes = |s: &BTreeSet<String>| Footprint {
            all: false,
            nodes: s.clone(),
        };
        for (agent, own) in scopes.iter().enumerate() {
            if own.is_empty() {
                signals
                    .uncertain
                    .push(format!("agent {agent} registered no scope"));
            }
            for o in obligations(db, agent)?.iter().filter(|o| o.pending()) {
                // Read a cause edit by its owners: an insertion names its file only for
                // relevance. Other causes stay uncertain (#311).
                let causes: Vec<Body> = all(
                    db,
                    "SELECT c.body FROM events o JOIN events c ON c.seq = json_extract(o.body, '$.Obligation.cause')
                     WHERE json_extract(o.body, '$.Obligation.agent')=? AND json_extract(o.body, '$.Obligation.scope')=?
                     AND json_extract(o.body, '$.Obligation.revision')>?",
                    params![agent as i64, o.scope.id, o.reviewed as i64],
                )?;
                let relevant = analysis::closure(&o.scope.relevant(), &sides);
                let uncertain = causes.is_empty()
                    || causes.iter().any(|cause| match cause {
                        Body::Message(Message {
                            kind: Kind::LiveEdit,
                            scope,
                            ..
                        }) => {
                            let mut footprint = Footprint::default();
                            owners(scope, &mut footprint);
                            analysis::related(&footprint, &relevant)
                        }
                        _ => true,
                    });
                if !uncertain {
                    continue;
                }
                signals.uncertain.push(format!(
                    "agent {agent} has not reviewed {} at revision {}",
                    o.scope.id, o.required
                ));
            }
        }
        for a in &scopes[0] {
            for b in &scopes[1] {
                if analysis::related(&nodes(&a.relevant()), &nodes(&b.nodes))
                    || analysis::related(&nodes(&b.relevant()), &nodes(&a.nodes))
                {
                    signals.declared.push(format!("0:{} 1:{}", a.id, b.id));
                }
            }
        }
        let window = window(db, mark)?;
        signals.revised = window.revised;
        // After an edit or a transition, an agent without drafts has no current work (#357).
        let work = [0, 1].map(|agent| {
            if drafts[agent].all || !drafts[agent].nodes.is_empty() {
                vec![(DRAFTS.to_string(), drafts[agent].clone())]
            } else if mark.event == 0 && !window.edited[agent] {
                scopes[agent]
                    .iter()
                    .map(|s| (s.id.clone(), nodes(&s.nodes)))
                    .collect()
            } else {
                Vec::new()
            }
        });
        for (la, a) in &work[0] {
            for (lb, b) in &work[1] {
                let pair = format!("0:{la} 1:{lb}");
                let (left, right) = (
                    analysis::closure(&a.nodes, &sides),
                    analysis::closure(&b.nodes, &sides),
                );
                if a.all || b.all || left.all || right.all {
                    signals
                        .uncertain
                        .push(format!("unbounded relevance for {pair}"));
                    continue;
                }
                if analysis::related(a, b) {
                    signals.overlap.push(pair.clone());
                }
                if analysis::related(&left, b) || analysis::related(&right, a) {
                    signals.compiler.push(pair);
                }
            }
        }
        let tasks = |s: &[Scope]| s.iter().map(|s| s.task.clone()).collect::<BTreeSet<_>>();
        signals.shared_tasks = tasks(&scopes[0])
            .intersection(&tasks(&scopes[1]))
            .cloned()
            .collect();
        signals.retries = db.query_row(
            "SELECT COUNT(*) FROM requests WHERE id>? AND json_extract(body, '$.outcome.Stale') IS NOT NULL
             AND json_extract(body, '$.before.revision')>=?",
            params![mark.request as i64, window.since as i64],
            |r| r.get::<_, i64>(0),
        )? as u64;
        signals.reconsiderations = db.query_row(
            "SELECT COUNT(*) FROM events o JOIN events c ON c.seq = json_extract(o.body, '$.Obligation.cause')
             WHERE o.seq>? AND json_extract(c.body, '$.Message') IS NOT NULL",
            [mark.event as i64],
            |r| r.get::<_, i64>(0),
        )? as u64;
        signals.waits = db.query_row(
            "SELECT COUNT(*) FROM waits WHERE id>?",
            [mark.wait as i64],
            |r| r.get::<_, i64>(0),
        )? as u64;
        let split = layout.placement[0] != layout.placement[1];
        let available: Vec<(usize, u64, String)> = all(
            db,
            "SELECT json_array(json_extract(body, '$.agent'), json_extract(body, '$.capture.revision'), json_extract(body, '$.id'))
             FROM checkpoints WHERE json_extract(body, '$.state')='Available' ORDER BY id",
            [],
        )?;
        for (agent, revision, id) in available {
            if layout.current(layout.placement[agent])?.revision == revision {
                signals.pending.push(id);
            }
        }
        let last = history
            .iter()
            .rev()
            .find(|(_, p)| matches!(p.state, ProposalState::Applied { .. }));
        if split && let Some((_, last)) = last {
            for (agent, own) in scopes.iter().enumerate() {
                for scope in own {
                    let before = last.scopes[agent].iter().find(|s| s.id == scope.id);
                    if before.map_or(!scope.depends.is_empty(), |b| b.depends != scope.depends) {
                        signals.redeclared.push(format!("{agent}:{}", scope.id));
                    }
                }
            }
            let behind: Vec<(u64, Body)> = all(
                db,
                "SELECT json_array(seq, json(body)) FROM events WHERE seq>? AND json_extract(body, '$.Behind') IS NOT NULL",
                [mark.event as i64],
            )?;
            for (_, body) in behind {
                if let Body::Behind { overlaps, .. } = body {
                    signals.behind.extend(overlaps);
                }
            }
            signals.behind.sort();
            signals.behind.dedup();
        }
        for (seq, earlier) in history {
            if *seq > mark.proposal
                && let Some(agent) = earlier.proposer
            {
                signals
                    .reports
                    .push(format!("agent {agent}: {}", earlier.reasoning));
                // A declined report is answered: disagreement keeps the arrangement (#14, #358).
                signals.failing |= earlier.signals.failing
                    && !matches!(earlier.state, ProposalState::Declined { .. });
            }
        }
        Ok((signals, stranding(&drafts, &sides)))
    }

    fn recommendation(
        &self,
        proposer: Option<usize>,
        propose: Option<Propose>,
    ) -> Result<RegroupingProposal> {
        let proposal = {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            let layout = self.layout()?;
            let scopes = scopes(&db)?;
            let history = proposals(&db)?;
            let mark = mark(&db)?;
            let (mut signals, stranded) = self.signals(&db, &layout, &scopes, &history, &mark)?;
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
            // A split moves every unpublished draft: never apart from one it uses (#357).
            if action == Action::Split
                && let Some(edge) = stranded
            {
                action = Action::Keep;
                reasoning = format!("suppressed Split: {UNSAFE}: {edge}; {reasoning}");
            }
            // Reason-to-change rule: no reversal or repeat without new evidence or a failure report.
            let last = history
                .iter()
                .rev()
                .find(|(_, p)| matches!(p.state, ProposalState::Applied { .. }));
            if let Some((_, last)) = last
                && action != Action::Keep
            {
                // shortcut: compiler pairs compare by label, and `0:drafts 1:drafts` reads the
                // same whatever the drafts hold; compare owning symbols if a changed edge
                // between drafts must count as new. A compiler edge alone never joins (#14).
                let new = signals.failing
                    || last.scopes != scopes
                    || last.signals.compiler != signals.compiler
                    || last.signals.declared != signals.declared
                    || (action == Action::Join && signals.rejoining());
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
                    // ponytail: two agents and agent 1 always leaves; choose the mover by
                    // draft ownership if more agents or asymmetric moves matter.
                    let stay = layout.placement[0];
                    let free = (0..layout.roots.len())
                        .find(|s| *s != stay && !layout.placement.contains(s))
                        .unwrap_or(layout.roots.len());
                    [stay, free]
                }
            };
            let context = context(&db, &scopes)?;
            // An unchanged proposal from the same proposer since the last transition, including a
            // declined one, is not repeated.
            // A pending proposal with the same exact change stands, whoever proposed it.
            if let Some((_, pending)) = history.iter().find(|(_, p)| {
                p.state.pending()
                    && p.action == action
                    && p.target == target
                    && p.context == context
            }) {
                return Ok(pending.clone());
            }
            let since = mark.proposal;
            // A split voided as unsafe is proposed again once it is safe (#357).
            // shortcut: the Stale cause is told apart by its reason prefix; type the cause if
            // another rule must read it.
            let unsafe_split = |p: &RegroupingProposal| matches!(&p.state, ProposalState::Stale { reason } if reason.starts_with(UNSAFE));
            if let Some((_, previous)) = history.iter().rev().find(|(seq, p)| {
                *seq > since
                    && !unsafe_split(p)
                    && p.proposer == proposer
                    && p.action == action
                    && p.target == target
                    && p.context == context
                    && material(&p.signals) == material(&signals)
            }) {
                return Ok(previous.clone());
            }
            let seq = scalar(&db, "SELECT COALESCE(MAX(seq), 0) + 1 FROM proposals")?;
            let proposal = RegroupingProposal {
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
                    if earlier.state.pending() {
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
            // Engine-made keeps are recorded and returned, not announced.
            if action != Action::Keep || proposer.is_some() {
                announce(&tx, &proposal)?;
            }
            tx.commit()?;
            proposal
        };
        self.signal.notify();
        Ok(proposal)
    }

    /// After a committed operation: its own outcome stands. Failed transitions are
    /// recorded as blockers, interrupted installations halt durably, and anything else
    /// is retried at the next boundary.
    pub(crate) fn boundary(&self) {
        let _ = self.advance();
    }
    /// Voids proposals whose context changed, applies agreed ones whose blockers cleared,
    /// brings publications into split workspaces and, once enabled, reconsiders.
    fn advance(&self) -> Result<()> {
        let enabled = {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            let mut changed = false;
            for (_, mut proposal) in proposals(&db)? {
                if !proposal.state.pending() {
                    continue;
                }
                if current_context(&db)? != proposal.context {
                    proposal.state = ProposalState::Stale {
                        reason: CONTEXT_CHANGED.into(),
                    };
                    commit_state(&db, &proposal)?;
                    changed = true;
                } else if proposal.agreed == proposal.agents {
                    changed |= match self.attempt(&db, &mut proposal) {
                        // Outside the blocker mapping: the transition is already committed.
                        Ok(changed) => {
                            if matches!(proposal.state, ProposalState::Applied { .. }) {
                                self.fault(Fault::Committed)?;
                            }
                            changed
                        }
                        Err(error) => {
                            proposal.state = ProposalState::Blocked {
                                blocker: format!("transition failed: {error}"),
                            };
                            commit_state(&db, &proposal)?;
                            true
                        }
                    };
                }
            }
            changed |= self.synchronize(&db)?;
            if changed {
                self.signal.notify();
            }
            meta(&db, "regroup_enabled")?.is_some()
        };
        // ponytail: reconsiders at every boundary; unchanged evidence is suppressed, and
        // analysis is cached per tree. Throttle if broken drafts make re-analysis slow.
        if enabled {
            self.recommendation(None, None)?;
        }
        Ok(())
    }

    /// Independent safety checks. A refusal is the state to record: `Blocked` naming the
    /// blocker, or `Stale` for a split that unpublished drafts make unsafe (#357).
    fn plan(&self, db: &Connection, layout: &Layout, proposal: &RegroupingProposal) -> Planned {
        if let Some(halt) = halted(db)? {
            return blocked(format!("engine halted: {halt}"));
        }
        if let Some(run) = publication::publishing(db)? {
            return blocked(format!("publication run {} in progress", run.id));
        }
        let published = revision(db, published(db)?)?;
        for space in proposal.from {
            if !layout
                .current(space)?
                .included
                .is_superset(&published.included)
            {
                return blocked(format!(
                    "workspace {space} has not incorporated publication {}",
                    published.revision
                ));
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
        let grouped = |space: usize| -> BTreeSet<usize> {
            (0..2).filter(|a| proposal.target[*a] == space).collect()
        };
        let mut moves = Vec::new();
        match proposal.action {
            Action::Keep => return blocked("a keep proposal has nothing to apply".into()),
            Action::Split => {
                let (stay, target) = (proposal.from[0], proposal.target[1]);
                if target >= layout.roots.len() {
                    return blocked(format!("workspace root {target} is not provisioned"));
                }
                if layout.placement.contains(&target) {
                    return blocked(format!("workspace {target} is occupied"));
                }
                let workspace = layout.current(stay)?;
                let [kept, moved] = match drafts(workspace)? {
                    Ok(own) => own,
                    Err(blocker) => return blocked(blocker),
                };
                // shortcut: drafts move per file, so two authors in one file void the split until
                // publication; move them per hunk if that keeps pairs joined too long.
                // Reverted drafts drop out, as in `signals` (#357).
                let differ = differing(&published, workspace);
                let interleaved: Vec<_> = kept
                    .intersection(&moved)
                    .filter(|p| differ.contains(*p))
                    .collect();
                if !interleaved.is_empty() {
                    return unsafe_until_published(format!(
                        "interleaved unpublished drafts in {interleaved:?}"
                    ));
                }
                // Required, not a backstop: drafts can change between proposal and application.
                let evidence = [
                    self.evidence_in(db, &published)?,
                    self.evidence_in(db, workspace)?,
                ];
                let footprints = drafted(
                    [&kept, &moved],
                    &published,
                    workspace,
                    [&evidence[0], &evidence[1]],
                );
                if let Some(edge) = stranding(&footprints, &[&evidence[1]]) {
                    return unsafe_until_published(edge);
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
                created.group = grouped(target);
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
                    remaining.group = grouped(stay);
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
                let (host_drafts, guest_drafts) = match (drafts(host)?, drafts(guest)?) {
                    (Ok([a, b]), Ok([c, d])) => (
                        a.union(&b).cloned().collect::<BTreeSet<_>>(),
                        c.union(&d).cloned().collect::<BTreeSet<_>>(),
                    ),
                    (Err(blocker), _) | (_, Err(blocker)) => return blocked(blocker),
                };
                let overlap: Vec<_> = host_drafts.intersection(&guest_drafts).collect();
                if !overlap.is_empty() {
                    // A join waits for the publication instead: agreement to it still holds.
                    return blocked(format!(
                        "both workspaces hold unpublished drafts of {overlap:?}"
                    ));
                }
                if !guest_drafts.is_empty() {
                    let mut joined = host.clone();
                    joined.revision = next;
                    joined.group = grouped(into);
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
    fn attempt(&self, db: &Connection, proposal: &mut RegroupingProposal) -> Result<bool> {
        // Copied out: no layout guard is held while the fault hook runs.
        let layout = self.layout()?.clone();
        let moves = match self.plan(db, &layout, proposal)? {
            Ok(moves) => moves,
            Err(refused) => {
                if proposal.state == refused {
                    return Ok(false);
                }
                proposal.state = refused;
                commit_state(db, proposal)?;
                return Ok(true);
            }
        };
        // Each agent's view: its current workspace before, its target workspace after.
        let mut views = Vec::new();
        for agent in 0..2 {
            let before = layout.current(layout.of(agent))?;
            let target = proposal.target[agent];
            let after = match moves.iter().find(|(s, _)| *s == target) {
                Some((_, capture)) => capture,
                None => layout.current(target)?,
            };
            let paths = differing(before, after);
            if !paths.is_empty() {
                views.push((
                    agent,
                    after.revision,
                    self.view_change(db, before, after, &paths)?,
                ));
            }
        }
        let shift = self.install_moves(db, &layout, &moves)?;
        proposal.state = ProposalState::Applied {
            revisions: moves.iter().map(|(s, c)| (*s, c.revision)).collect(),
        };
        let committed = (|| -> Result<()> {
            let tx = db.unchecked_transaction()?;
            save(&tx, proposal)?;
            let event = announce(&tx, proposal)?;
            self.commit_moves(&tx, shift, &moves, proposal.target)?;
            for (agent, revision, view) in &views {
                self.renew_for_view(&tx, *agent, view, *revision, event)?;
            }
            let mark = Mark {
                event,
                request: scalar(&tx, "SELECT COALESCE(MAX(id), 0) FROM requests")?,
                wait: scalar(&tx, "SELECT COALESCE(MAX(id), 0) FROM waits")?,
                proposal: scalar(&tx, "SELECT COALESCE(MAX(seq), 0) FROM proposals")?,
            };
            set_meta(&tx, "regroup_mark", &serde_json::to_string(&mark)?)?;
            tx.commit()?;
            Ok(())
        })();
        interrupted(db, shift, committed)?;
        self.adopt(moves, proposal.target)?;
        Ok(true)
    }

    /// Brings each occupied workspace up to the published state when its own drafts allow.
    fn synchronize(&self, db: &Connection) -> Result<bool> {
        let published = revision(db, published(db)?)?;
        let mut changed = false;
        let occupied: BTreeSet<usize> = self.layout()?.placement.into_iter().collect();
        for space in &occupied {
            // Copied out: no layout guard is held while the fault hook runs.
            let layout = {
                let layout = self.layout()?;
                if layout
                    .current(*space)?
                    .included
                    .is_superset(&published.included)
                {
                    continue;
                }
                layout.clone()
            };
            let workspace = layout.current(*space)?;
            let members = layout.group(*space);
            let audience: i64 = members.iter().map(|a| crate::coordination::mine(*a)).sum();
            let (mut merged, overlaps) = merge(db, workspace, &published)?;
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
            if halted(db)?.is_some() {
                continue;
            }
            merged.revision = next_revision(db)?;
            merged.group = layout.group(*space);
            merged.tree = build_tree(&self.state, &merged.files, "")?;
            let moves = vec![(*space, merged.clone())];
            let view = self.view_change(db, workspace, &merged, &differing(workspace, &merged))?;
            let shift = self.install_moves(db, &layout, &moves)?;
            let placement = layout.placement;
            let committed = (|| -> Result<()> {
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
                self.commit_moves(&tx, shift, &moves, placement)?;
                for agent in members {
                    self.renew_for_view(&tx, agent, &view, merged.revision, event)?;
                }
                tx.commit()?;
                Ok(())
            })();
            interrupted(db, shift, committed)?;
            self.adopt(moves, placement)?;
            self.fault(Fault::Committed)?;
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
        let shift = Installation {
            moves: moves
                .iter()
                .map(|(space, after)| RootChange {
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
                None => ensure_empty(root),
            };
            if let Err(error) = verified {
                self.save_incident(db, &layout.roots, &error.to_string())?;
                return Err(error);
            }
            retain(&self.state, &change.after)?;
        }
        self.fault(Fault::Journaling)?;
        db.execute(
            "INSERT INTO shifts(body) VALUES(?)",
            [serde_json::to_string(&shift)?],
        )?;
        let id = db.last_insert_rowid();
        let installed = (|| -> Result<()> {
            self.fault(Fault::Journaled)?;
            for change in &shift.moves {
                let root = &layout.roots[change.space];
                for (path, file) in &change.after.files {
                    let before = change.before.as_ref().map(|b| &b.files[path].file);
                    if before != Some(&file.file) {
                        install(root, path, &file.file)?;
                        self.fault(Fault::Installed {
                            space: change.space,
                            path: path.clone(),
                        })?;
                    }
                }
                audit(root, &change.after)?;
                self.fault(Fault::Audited {
                    space: change.space,
                })?;
            }
            Ok(())
        })();
        interrupted(db, id, installed)?;
        Ok(id)
    }
    /// Inside the caller's transaction: new revisions, pointers, placement, installed shift.
    fn commit_moves(
        &self,
        tx: &Connection,
        shift: i64,
        moves: &[(usize, Capture)],
        placement: [usize; 2],
    ) -> Result<()> {
        for (space, capture) in moves {
            tx.execute(
                "INSERT INTO revisions VALUES(?,?)",
                params![capture.revision as i64, serde_json::to_string(capture)?],
            )?;
            tx.execute(
                "UPDATE spaces SET completed=? WHERE id=?",
                params![capture.revision as i64, *space as i64],
            )?;
        }
        set_meta(tx, "placement", &serde_json::to_string(&placement)?)?;
        let mut recorded: Installation =
            one(tx, "SELECT body FROM shifts WHERE id=?", [shift])?.ok_or("unknown shift")?;
        recorded.installing = false;
        tx.execute(
            "UPDATE shifts SET body=? WHERE id=?",
            params![serde_json::to_string(&recorded)?, shift],
        )?;
        self.fault(Fault::Commit)
    }
    /// After the commit: the layout takes the committed revisions and placement.
    fn adopt(&self, moves: Vec<(usize, Capture)>, placement: [usize; 2]) -> Result<()> {
        let mut layout = self.layout.write().map_err(|_| "layout lock poisoned")?;
        for (space, capture) in moves {
            layout.completed[space] = Some(capture);
        }
        layout.placement = placement;
        Ok(())
    }
}

type Planned = Result<std::result::Result<Vec<(usize, Capture)>, ProposalState>>;
/// The arrangement waits; agreement holds and the proposal is retried at boundaries.
fn blocked(blocker: String) -> Planned {
    Ok(Err(ProposalState::Blocked { blocker }))
}
/// A split refused because of unpublished drafts: agreement is void, and the engine
/// proposes it again once a publication makes it safe (#357).
fn unsafe_until_published(reason: String) -> Planned {
    Ok(Err(ProposalState::Stale {
        reason: format!("{UNSAFE}: {reason}"),
    }))
}

/// A failure once `shift` is recorded as installing, before its commit, leaves files the
/// engine wrote: halt durably until restart restores the before state.
fn interrupted<T>(db: &Connection, shift: i64, result: Result<T>) -> Result<T> {
    if let Err(error) = &result {
        set_halt(
            db,
            &Halt::Interruption {
                installation: Interrupted::Transition(shift),
                reason: error.to_string(),
            },
        )?;
    }
    result
}

/// The recency window: the latest settled review in each direction since the last
/// applied transition. It bounds collaboration evidence, not current work (#357).
struct Window {
    /// Per agent, whether it has edited since the last applied transition.
    edited: [bool; 2],
    /// Latest settled reviews that decided `Revise`.
    revised: Vec<String>,
    /// The older of the two latest reviewed revisions; stale refusals since it count.
    since: u64,
}
fn window(db: &Connection, mark: &Mark) -> Result<Window> {
    let rows: Vec<(u64, Body)> = all(
        db,
        "SELECT json_array(seq, json(body)) FROM events WHERE seq>? AND
         (json_extract(body, '$.Reviewed') IS NOT NULL OR json_extract(body, '$.Message.kind')='LiveEdit')
         ORDER BY seq",
        [mark.event as i64],
    )?;
    let mut reviews: [BTreeMap<u64, Vec<Review>>; 2] = Default::default();
    let mut window = Window {
        edited: [false, false],
        revised: Vec::new(),
        since: u64::MAX,
    };
    for (_, body) in rows {
        match body {
            Body::Reviewed { agent, review } => reviews[agent]
                .entry(review.revision)
                .or_default()
                .push(review),
            Body::Message(Message {
                kind: Kind::LiveEdit,
                sender,
                ..
            }) => window.edited[sender] = true,
            _ => {}
        }
    }
    for (reviewer, reviewed) in reviews.iter().enumerate() {
        let current = reviewed.iter().next_back();
        window.since = window.since.min(current.map_or(0, |(r, _)| *r));
        for review in current.into_iter().flat_map(|(_, r)| r) {
            if review.decision == Decision::Revise {
                window.revised.push(format!(
                    "agent {reviewer} revised {} at revision {}",
                    review.scope, review.revision
                ));
            }
        }
    }
    Ok(window)
}
/// The owning symbols a live edit names. An insertion between declarations also names
/// its file for relevance; its owner is the new declaration.
fn owners(scope: &[String], footprint: &mut Footprint) {
    for node in scope {
        if node == "*" {
            footprint.all = true;
        } else if !scope.iter().any(|n| {
            n.strip_prefix(node.as_str())
                .is_some_and(|rest| rest.starts_with('#'))
        }) {
            footprint.nodes.insert(node.clone());
        }
    }
}

/// Evidence that changes a recommendation's meaning; counts only matter as presence.
type Material = (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    bool,
    bool,
    bool,
);
fn material(signals: &Signals) -> Material {
    (
        signals.compiler.clone(),
        signals.overlap.clone(),
        signals.revised.clone(),
        signals.redeclared.clone(),
        signals.behind.clone(),
        signals.pending.clone(),
        signals.retries > 0,
        signals.failing,
        signals.uncertain.is_empty(),
    )
}

/// Keep joined agents together while they collaborate now, split them when their current
/// work is independent, and keep the arrangement when uncertain. Split agents join only on
/// overlap, a new declaration, an overlapping publication or a failure report.
fn decide(joined: bool, signals: &Signals) -> (Action, String) {
    let summary = format!(
        "overlap {:?}, compiler {:?}, revised {:?}, stale refusals {}, redeclared {:?}, behind {:?}",
        signals.overlap,
        signals.compiler,
        signals.revised,
        signals.retries,
        signals.redeclared,
        signals.behind
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
        if signals.collaborating() {
            return (Action::Keep, format!("current collaboration; {summary}"));
        }
        // The rule lifts itself: the next accepted edit advances the revision past the offer.
        if !signals.pending.is_empty() {
            return (
                Action::Keep,
                format!("pending offer {:?}; {summary}", signals.pending),
            );
        }
        return (
            Action::Split,
            format!("current work looks independent; separate progress is useful; {summary}"),
        );
    }
    if signals.joining() {
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
