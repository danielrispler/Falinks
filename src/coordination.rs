//! Registered scopes, revision-bound obligations, explicit reviews, typed messages,
//! checkpoint offers, the durable event stream and voluntary waits.
use crate::analysis::{self, Evidence, Footprint};
use crate::{Capture, Client, Engine, Request, Result, connect, fail, meta, retain};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Intended work registered before substantial reasoning. Nodes are enrolled
/// paths or `path#owner` symbols from analysis evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub id: String,
    pub task: String,
    pub nodes: BTreeSet<String>,
}
/// Pending while `required` (the latest relevant completed revision) exceeds
/// `reviewed` (the latest explicitly reviewed captured revision).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obligation {
    pub agent: usize,
    pub scope: Scope,
    pub required: u64,
    pub reviewed: u64,
}
impl Obligation {
    pub fn pending(&self) -> bool {
        self.required > self.reviewed
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Keep,
    Revise,
    Drop,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub scope: String,
    /// A captured completed revision the agent reread.
    pub revision: u64,
    pub decision: Decision,
    pub note: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Plan,
    LiveEdit,
    CheckpointOffer,
    Publication,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Work {
    None,
    Revision(u64),
    Checkpoint(String),
}
/// Binds a reply to the original event and the exact work that event referred to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub event: u64,
    pub work: Work,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub kind: Kind,
    pub sender: usize,
    pub workspace: String,
    pub task: String,
    pub scope: Vec<String>,
    pub work: Work,
    pub text: String,
    pub reply_to: Option<Reply>,
}
/// A client-posted plan: advisory, reserves nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub task: String,
    pub scope: Vec<String>,
    pub work: Work,
    pub text: String,
    pub reply_to: Option<Reply>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub id: String,
    pub revision: u64,
    pub scope: Vec<String>,
    pub text: String,
    pub supersedes: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Availability {
    Available,
    Withdrawn,
    Superseded { replacement: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// `agent/offer-id`.
    pub id: String,
    pub agent: usize,
    pub capture: Capture,
    pub offer: Offer,
    pub state: Availability,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Body {
    Message(Message),
    /// A relevant change created or renewed a review requirement.
    Obligation {
        agent: usize,
        scope: String,
        revision: u64,
        cause: u64,
    },
    Reviewed {
        agent: usize,
        review: Review,
    },
    Withdrawn {
        checkpoint: String,
    },
    Superseded {
        checkpoint: String,
        replacement: String,
    },
    Disconnected {
        agent: usize,
    },
}
/// `seq` is the SQLite commit sequence; `id` is stable across replay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub id: String,
    pub body: Body,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Disposition {
    Processed,
    Deferred { note: String },
}
/// Everything a reconnecting client must restore before related writes/offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub handled: u64,
    pub unhandled: Vec<Event>,
    pub deferred: Vec<(Event, String)>,
    pub obligations: Vec<Obligation>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Condition {
    /// The next message from `from` after stream position `after`.
    Message { from: usize, after: u64 },
    /// Acceptance of this exact checkpoint. Publication is not enabled in this
    /// slice, so such a wait ends only through the explicit non-met outcomes.
    Checkpoint { id: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WaitOutcome {
    Met(Event),
    TimedOut,
    Cancelled,
    Disconnected {
        agent: usize,
    },
    Withdrawn {
        checkpoint: String,
    },
    /// Names the replacement; the wait is never rebound automatically.
    Superseded {
        checkpoint: String,
        replacement: String,
    },
}

pub(crate) struct Signal {
    connected: Mutex<[bool; 2]>,
    wake: Condvar,
}
impl Signal {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            connected: Mutex::new([false; 2]),
            wake: Condvar::new(),
        })
    }
    /// Attention request: only after the durable commit. Waiters reread the store.
    pub(crate) fn notify(&self) {
        let _guard = self.connected.lock();
        self.wake.notify_all();
    }
    pub(crate) fn presence(&self, agent: usize, connected: bool) {
        if let Ok(mut state) = self.connected.lock() {
            state[agent] = connected;
        }
        self.wake.notify_all();
    }
}
#[derive(Clone)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    signal: Arc<Signal>,
}
impl Cancel {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.signal.notify();
    }
}

pub(crate) fn tables(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS evidence (tree TEXT NOT NULL, toolchain TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(tree, toolchain));
        CREATE TABLE IF NOT EXISTS scopes (agent INTEGER NOT NULL, id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(agent, id));
        CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY AUTOINCREMENT, audience INTEGER NOT NULL, body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS cursors (agent INTEGER PRIMARY KEY, seq INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS deferred (agent INTEGER NOT NULL, seq INTEGER NOT NULL, note TEXT NOT NULL, PRIMARY KEY(agent, seq));
        CREATE TABLE IF NOT EXISTS checkpoints (id TEXT PRIMARY KEY, body TEXT NOT NULL);",
    )?;
    Ok(())
}
fn others(agent: usize) -> i64 {
    0b11 & !(1 << agent)
}
fn mine(agent: usize) -> i64 {
    1 << agent
}
fn obligations(db: &Connection, agent: usize) -> Result<Vec<Obligation>> {
    let mut stmt = db.prepare("SELECT body FROM scopes WHERE agent=? ORDER BY id")?;
    let rows = stmt.query_map([agent as i64], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
fn save_obligation(db: &Connection, obligation: &Obligation) -> Result<()> {
    db.execute(
        "INSERT INTO scopes VALUES(?,?,?) ON CONFLICT(agent,id) DO UPDATE SET body=excluded.body",
        params![
            obligation.agent as i64,
            obligation.scope.id,
            serde_json::to_string(obligation)?
        ],
    )?;
    Ok(())
}
fn event(db: &Connection, workspace: &str, seq: u64) -> Result<Option<Event>> {
    db.query_row("SELECT body FROM events WHERE seq=?", [seq as i64], |r| {
        r.get::<_, String>(0)
    })
    .optional()?
    .map(|body| {
        Ok(Event {
            seq,
            id: format!("{workspace}:{seq}"),
            body: serde_json::from_str(&body)?,
        })
    })
    .transpose()
}
fn events_after(db: &Connection, workspace: &str, agent: usize, after: u64) -> Result<Vec<Event>> {
    let mut stmt =
        db.prepare("SELECT seq, body FROM events WHERE seq>? AND audience & ? != 0 ORDER BY seq")?;
    let rows = stmt.query_map(params![after as i64, mine(agent)], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (seq, body) = row?;
        Ok(Event {
            seq: seq as u64,
            id: format!("{workspace}:{seq}"),
            body: serde_json::from_str(&body)?,
        })
    })
    .collect()
}
fn emit(db: &Connection, audience: i64, body: &Body) -> Result<u64> {
    db.execute(
        "INSERT INTO events(audience, body) VALUES(?,?)",
        params![audience, serde_json::to_string(body)?],
    )?;
    Ok(db.last_insert_rowid() as u64)
}
fn cursor(db: &Connection, agent: usize) -> Result<u64> {
    Ok(db
        .query_row(
            "SELECT seq FROM cursors WHERE agent=?",
            [agent as i64],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0) as u64)
}
fn checkpoint(db: &Connection, id: &str) -> Result<Option<Checkpoint>> {
    db.query_row("SELECT body FROM checkpoints WHERE id=?", [id], |r| {
        r.get::<_, String>(0)
    })
    .optional()?
    .map(|b| Ok(serde_json::from_str(&b)?))
    .transpose()
}
fn save_checkpoint(db: &Connection, checkpoint: &Checkpoint) -> Result<()> {
    db.execute(
        "INSERT INTO checkpoints VALUES(?,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
        params![checkpoint.id, serde_json::to_string(checkpoint)?],
    )?;
    Ok(())
}
fn captured(db: &Connection, revision: u64) -> Result<Option<Capture>> {
    // Only a coherent capture an agent was actually given can be reviewed or offered.
    let seen = db
        .query_row(
            "SELECT 1 FROM captures WHERE json_extract(body, '$.revision')=? LIMIT 1",
            [revision as i64],
            |_| Ok(()),
        )
        .optional()?;
    if seen.is_none() {
        return Ok(None);
    }
    db.query_row(
        "SELECT body FROM revisions WHERE id=?",
        [revision as i64],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .map(|b| Ok(serde_json::from_str(&b)?))
    .transpose()
}

/// Footprint of a proposed operation, carried from the gate to the commit.
pub(crate) struct Change {
    footprint: Footprint,
    evidence: [Vec<Evidence>; 2],
}

impl Engine {
    /// Host configuration: verify pins and analyze the current completed revision.
    /// Not durable; after restart relevance stays conservative until reconfigured.
    pub fn configure_analysis(
        &self,
        toolchains: Vec<analysis::Toolchain>,
    ) -> Result<Vec<Evidence>> {
        for toolchain in &toolchains {
            analysis::verify(toolchain)?;
        }
        *self
            .analysis
            .write()
            .map_err(|_| "analysis lock poisoned")? = toolchains;
        let capture = self.capture()?;
        self.evidence(&capture)
    }
    /// Evidence for an exact capture under every configured toolchain, analyzing on demand.
    pub fn evidence(&self, capture: &Capture) -> Result<Vec<Evidence>> {
        let toolchains = self
            .analysis
            .read()
            .map_err(|_| "analysis lock poisoned")?
            .clone();
        let db = connect(&self.state)?;
        let mut all = Vec::new();
        for toolchain in &toolchains {
            let key = analysis::toolchain_key(toolchain)?;
            let cached = db
                .query_row(
                    "SELECT body FROM evidence WHERE tree=? AND toolchain=?",
                    params![capture.tree, key],
                    |r| r.get::<_, String>(0),
                )
                .optional()?;
            let evidence: Evidence = match cached {
                Some(body) => serde_json::from_str(&body)?,
                None => {
                    let scratch = self.state.join("analysis").join(crate::nonce()?);
                    std::fs::create_dir_all(&scratch)?;
                    let evidence = analysis::analyze(&scratch, capture, toolchain);
                    // Infrastructure failures are retained once for diagnosis, never cached as truth.
                    if evidence.errors.is_empty() {
                        let _ = std::fs::remove_dir_all(&scratch);
                        db.execute(
                            "INSERT OR IGNORE INTO evidence VALUES(?,?,?)",
                            params![capture.tree, key, serde_json::to_string(&evidence)?],
                        )?;
                    }
                    evidence
                }
            };
            all.push(evidence);
        }
        Ok(all)
    }

    /// Writes and offers related to a pending obligation are blocked until review.
    pub(crate) fn gate(
        &self,
        db: &Connection,
        client: &Client,
        before: &Capture,
        proposed: &Capture,
        request: &Request,
    ) -> Result<std::result::Result<Change, crate::Outcome>> {
        let configured = !self
            .analysis
            .read()
            .map_err(|_| "analysis lock poisoned")?
            .is_empty();
        // ponytail: analysis runs inside the short writer operation; pre-analyze
        // proposals outside the lock if real edit throughput suffers.
        let evidence = [self.evidence(before)?, self.evidence(proposed)?];
        let unknown: Vec<_> = evidence
            .iter()
            .flatten()
            .flat_map(|e| e.unknown.iter().cloned())
            .collect();
        if configured && !unknown.is_empty() {
            return Ok(Err(crate::Outcome::Rejected {
                reason: format!("unknown analysis input universe; recapture required: {unknown:?}"),
            }));
        }
        let paths: BTreeSet<_> = request.output.keys().cloned().collect();
        let sides = [evidence[0].as_slice(), evidence[1].as_slice()];
        let footprint = analysis::change(before, proposed, &paths, &sides);
        let blocked: BTreeMap<_, _> = obligations(db, client.agent)?
            .into_iter()
            .filter(|o| o.pending())
            .filter(|o| analysis::related(&footprint, &analysis::closure(&o.scope.nodes, &sides)))
            .map(|o| (o.scope.id.clone(), o))
            .collect();
        if !blocked.is_empty() {
            return Ok(Err(crate::Outcome::Unreviewed {
                obligations: blocked,
            }));
        }
        Ok(Ok(Change {
            footprint,
            evidence,
        }))
    }

    /// Inside the completed-pointer transaction: the live-edit message and every
    /// renewed obligation commit together with the revision they describe.
    pub(crate) fn record_change(
        &self,
        tx: &Connection,
        client: &Client,
        request: &Request,
        proposed: &Capture,
        change: &Change,
    ) -> Result<()> {
        let task = obligations(tx, client.agent)?
            .iter()
            .map(|o| o.scope.task.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let cause = emit(
            tx,
            others(client.agent),
            &Body::Message(Message {
                kind: Kind::LiveEdit,
                sender: client.agent,
                workspace: self.identity.workspace.clone(),
                task,
                scope: if change.footprint.all {
                    vec!["*".into()]
                } else {
                    change.footprint.nodes.iter().cloned().collect()
                },
                work: Work::Revision(proposed.revision),
                text: format!("request {}", request.id),
                reply_to: None,
            }),
        )?;
        let sides = [change.evidence[0].as_slice(), change.evidence[1].as_slice()];
        for agent in (0..2).filter(|a| *a != client.agent) {
            for mut obligation in obligations(tx, agent)? {
                if analysis::related(
                    &change.footprint,
                    &analysis::closure(&obligation.scope.nodes, &sides),
                ) {
                    obligation.required = proposed.revision;
                    save_obligation(tx, &obligation)?;
                    emit(
                        tx,
                        mine(agent),
                        &Body::Obligation {
                            agent,
                            scope: obligation.scope.id.clone(),
                            revision: proposed.revision,
                            cause,
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    pub fn register(&self, client: &Client, scope: Scope) -> Result<Obligation> {
        self.authorize(client)?;
        let completed = self
            .completed
            .read()
            .map_err(|_| "completed lock poisoned")?
            .clone();
        if scope.id.is_empty() || scope.nodes.is_empty() {
            return fail("scope needs an ID and at least one node");
        }
        for node in &scope.nodes {
            let path = node.split_once('#').map_or(node.as_str(), |(p, _)| p);
            if !completed.files.contains_key(path) {
                return fail(format!("scope node outside enrollment: {node}"));
            }
        }
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        // Updating a scope keeps its requirement; registration never clears work.
        let obligation = match obligations(&db, client.agent)?
            .into_iter()
            .find(|o| o.scope.id == scope.id)
        {
            Some(existing) => Obligation { scope, ..existing },
            None => Obligation {
                agent: client.agent,
                scope,
                required: 0,
                reviewed: completed.revision,
            },
        };
        save_obligation(&db, &obligation)?;
        Ok(obligation)
    }
    pub fn obligations(&self, client: &Client) -> Result<Vec<Obligation>> {
        self.authorize(client)?;
        obligations(&connect(&self.state)?, client.agent)
    }
    /// Records keep/revise/drop against a captured revision and clears only what it covers.
    pub fn review(&self, client: &Client, review: Review) -> Result<Obligation> {
        self.authorize(client)?;
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let mut obligation = obligations(&db, client.agent)?
            .into_iter()
            .find(|o| o.scope.id == review.scope)
            .ok_or("unknown scope")?;
        if captured(&db, review.revision)?.is_none() {
            return fail("review must name a revision captured for rereading");
        }
        obligation.reviewed = obligation.reviewed.max(review.revision);
        let tx = db.unchecked_transaction()?;
        save_obligation(&tx, &obligation)?;
        emit(
            &tx,
            others(client.agent),
            &Body::Reviewed {
                agent: client.agent,
                review,
            },
        )?;
        tx.commit()?;
        drop(db);
        self.signal.notify();
        Ok(obligation)
    }
    pub fn post(&self, client: &Client, plan: Plan) -> Result<Event> {
        self.authorize(client)?;
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        self.check_work(&db, &plan.work)?;
        if let Some(reply) = &plan.reply_to {
            match event(&db, &self.identity.workspace, reply.event)? {
                Some(Event {
                    body: Body::Message(original),
                    ..
                }) if original.work == reply.work => {}
                _ => return fail("reply must bind an existing message and its exact work"),
            }
        }
        let body = Body::Message(Message {
            kind: Kind::Plan,
            sender: client.agent,
            workspace: self.identity.workspace.clone(),
            task: plan.task,
            scope: plan.scope,
            work: plan.work,
            text: plan.text,
            reply_to: plan.reply_to,
        });
        let seq = emit(&db, others(client.agent), &body)?;
        drop(db);
        self.signal.notify();
        Ok(Event {
            seq,
            id: format!("{}:{seq}", self.identity.workspace),
            body,
        })
    }
    fn check_work(&self, db: &Connection, work: &Work) -> Result<()> {
        let known = match work {
            Work::None => true,
            Work::Revision(r) => meta(db, "completed")?.ok_or("completed")?.parse::<u64>()? >= *r,
            Work::Checkpoint(id) => checkpoint(db, id)?.is_some(),
        };
        if !known {
            return fail("message refers to unknown work");
        }
        Ok(())
    }
    /// Offers an exact captured revision. Unreviewed obligations at or before it block.
    pub fn offer(&self, client: &Client, offer: Offer) -> Result<Checkpoint> {
        self.authorize(client)?;
        let id = format!("{}/{}", client.agent, offer.id);
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        if let Some(existing) = checkpoint(&db, &id)? {
            if existing.offer != offer {
                return fail("offer ID reused with changed contents");
            }
            return Ok(existing);
        }
        let capture =
            captured(&db, offer.revision)?.ok_or("offer must name a captured revision")?;
        retain(&self.state, &capture)?;
        let pending: Vec<_> = obligations(&db, client.agent)?
            .into_iter()
            .filter(|o| o.pending() && o.required <= offer.revision)
            .map(|o| o.scope.id)
            .collect();
        if !pending.is_empty() {
            return fail(format!(
                "unreviewed obligations block the offer: {pending:?}"
            ));
        }
        let tx = db.unchecked_transaction()?;
        if let Some(old) = &offer.supersedes {
            let old = format!("{}/{old}", client.agent);
            let mut previous = checkpoint(&tx, &old)?.ok_or("unknown superseded checkpoint")?;
            if previous.state != Availability::Available {
                return fail("only an available checkpoint can be superseded");
            }
            previous.state = Availability::Superseded {
                replacement: id.clone(),
            };
            save_checkpoint(&tx, &previous)?;
            emit(
                &tx,
                others(client.agent),
                &Body::Superseded {
                    checkpoint: old,
                    replacement: id.clone(),
                },
            )?;
        }
        let created = Checkpoint {
            id: id.clone(),
            agent: client.agent,
            capture,
            offer: offer.clone(),
            state: Availability::Available,
        };
        save_checkpoint(&tx, &created)?;
        emit(
            &tx,
            others(client.agent),
            &Body::Message(Message {
                kind: Kind::CheckpointOffer,
                sender: client.agent,
                workspace: self.identity.workspace.clone(),
                task: String::new(),
                scope: offer.scope,
                work: Work::Checkpoint(id),
                text: offer.text,
                reply_to: None,
            }),
        )?;
        tx.commit()?;
        drop(db);
        self.signal.notify();
        Ok(created)
    }
    pub fn withdraw(&self, client: &Client, offer: &str) -> Result<Checkpoint> {
        self.authorize(client)?;
        let id = format!("{}/{offer}", client.agent);
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let mut existing = checkpoint(&db, &id)?.ok_or("unknown checkpoint")?;
        if existing.state != Availability::Available {
            return fail("checkpoint is not available");
        }
        existing.state = Availability::Withdrawn;
        let tx = db.unchecked_transaction()?;
        save_checkpoint(&tx, &existing)?;
        emit(
            &tx,
            others(client.agent),
            &Body::Withdrawn { checkpoint: id },
        )?;
        tx.commit()?;
        drop(db);
        self.signal.notify();
        Ok(existing)
    }
    pub fn checkpoint(&self, id: &str) -> Result<Option<Checkpoint>> {
        checkpoint(&connect(&self.state)?, id)
    }

    /// Complete ordered replay after `after`; duplicate delivery is harmless.
    pub fn events(&self, client: &Client, after: u64) -> Result<Vec<Event>> {
        self.authorize(client)?;
        events_after(
            &connect(&self.state)?,
            &self.identity.workspace,
            client.agent,
            after,
        )
    }
    /// Advance the durable handled position in order. Deferral is stored in the
    /// same transaction. Neither processing nor deferral touches obligations.
    pub fn handle(&self, client: &Client, seq: u64, disposition: Disposition) -> Result<()> {
        self.authorize(client)?;
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let handled = cursor(&db, client.agent)?;
        let tx = db.unchecked_transaction()?;
        if seq <= handled {
            if disposition == Disposition::Processed {
                tx.execute(
                    "DELETE FROM deferred WHERE agent=? AND seq=?",
                    params![client.agent as i64, seq as i64],
                )?;
            }
        } else {
            let next = events_after(&tx, &self.identity.workspace, client.agent, handled)?
                .first()
                .map(|e| e.seq);
            if next != Some(seq) {
                return fail(format!("handle events in order; next is {next:?}"));
            }
            if let Disposition::Deferred { note } = disposition {
                tx.execute(
                    "INSERT OR IGNORE INTO deferred VALUES(?,?,?)",
                    params![client.agent as i64, seq as i64, note],
                )?;
            }
            tx.execute(
                "INSERT INTO cursors VALUES(?,?) ON CONFLICT(agent) DO UPDATE SET seq=excluded.seq",
                params![client.agent as i64, seq as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn pending(&self, client: &Client) -> Result<Pending> {
        self.authorize(client)?;
        let db = connect(&self.state)?;
        let handled = cursor(&db, client.agent)?;
        let mut stmt = db.prepare("SELECT seq, note FROM deferred WHERE agent=? ORDER BY seq")?;
        let rows = stmt.query_map([client.agent as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut deferred = Vec::new();
        for row in rows {
            let (seq, note) = row?;
            let event = event(&db, &self.identity.workspace, seq as u64)?
                .ok_or("deferred event missing")?;
            deferred.push((event, note));
        }
        Ok(Pending {
            handled,
            unhandled: events_after(&db, &self.identity.workspace, client.agent, handled)?,
            deferred,
            obligations: obligations(&db, client.agent)?
                .into_iter()
                .filter(Obligation::pending)
                .collect(),
        })
    }

    /// Host runtime signal: a session ended. Waits on that peer end visibly.
    pub fn disconnect(&self, client: &Client) -> Result<()> {
        self.authorize(client)?;
        {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            emit(
                &db,
                others(client.agent),
                &Body::Disconnected {
                    agent: client.agent,
                },
            )?;
        }
        self.signal.presence(client.agent, false);
        Ok(())
    }
    pub fn cancel_token(&self) -> Cancel {
        Cancel {
            flag: Arc::new(AtomicBool::new(false)),
            signal: self.signal.clone(),
        }
    }
    /// Voluntary wait. Imposes no deadline on the peer; `timeout` is the caller's.
    pub fn wait(
        &self,
        client: &Client,
        condition: Condition,
        timeout: Option<Duration>,
        cancel: Option<&Cancel>,
    ) -> Result<WaitOutcome> {
        self.authorize(client)?;
        let deadline = timeout.map(|t| Instant::now() + t);
        // Check the durable store while holding the signal lock: a commit's
        // notification cannot fall between the check and the wait.
        let mut connected = self
            .signal
            .connected
            .lock()
            .map_err(|_| "signal poisoned")?;
        loop {
            if cancel.is_some_and(|c| c.flag.load(Ordering::SeqCst)) {
                return Ok(WaitOutcome::Cancelled);
            }
            let db = connect(&self.state)?;
            match &condition {
                Condition::Message { from, after } => {
                    let found = events_after(&db, &self.identity.workspace, client.agent, *after)?
                        .into_iter()
                        .find(|e| matches!(&e.body, Body::Message(m) if m.sender == *from));
                    if let Some(event) = found {
                        return Ok(WaitOutcome::Met(event));
                    }
                    if !connected.get(*from).copied().unwrap_or(false) {
                        return Ok(WaitOutcome::Disconnected { agent: *from });
                    }
                }
                Condition::Checkpoint { id } => {
                    let target = checkpoint(&db, id)?.ok_or("unknown checkpoint")?;
                    match target.state {
                        Availability::Withdrawn => {
                            return Ok(WaitOutcome::Withdrawn {
                                checkpoint: id.clone(),
                            });
                        }
                        Availability::Superseded { replacement } => {
                            return Ok(WaitOutcome::Superseded {
                                checkpoint: id.clone(),
                                replacement,
                            });
                        }
                        Availability::Available if !connected[target.agent] => {
                            return Ok(WaitOutcome::Disconnected {
                                agent: target.agent,
                            });
                        }
                        Availability::Available => {}
                    }
                }
            }
            connected = match deadline {
                None => self
                    .signal
                    .wake
                    .wait(connected)
                    .map_err(|_| "signal poisoned")?,
                Some(deadline) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Ok(WaitOutcome::TimedOut);
                    }
                    self.signal
                        .wake
                        .wait_timeout(connected, left)
                        .map_err(|_| "signal poisoned")?
                        .0
                }
            };
        }
    }
}
