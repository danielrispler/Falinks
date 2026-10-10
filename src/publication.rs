//! Exact team candidates, combined checks in the one reusable validation workspace,
//! and atomic SQLite publication.
use crate::coordination::{
    Availability, Body, captured, checkpoint, emit, obligations, save_checkpoint,
};
use crate::{
    Capture, Client, Engine, Outcome, Result, audit, connect, fail, install, meta, nonce, object,
    quote, read_file, records, retain, sandboxed, set_meta,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

/// A host-enrolled trusted check: compilation, fixed tests or another required command.
/// It runs in the validation workspace with read-only candidate source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}
/// Everything a result is bound to. Any difference requires fresh offers and checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub revision: u64,
    pub tree: String,
    /// The expected published revision.
    pub base: u64,
    pub members: BTreeSet<usize>,
    pub contributions: Vec<u64>,
    /// Covering checkpoint per member.
    pub checkpoints: BTreeMap<usize, String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub binding: Binding,
    /// Members without a matching available offer; their work blocks the candidate.
    pub missing: BTreeSet<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunKind {
    /// Queued automatically by complete offer coverage.
    Publication,
    /// Requested early by a client; never publishes.
    Feedback { agent: usize },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub status: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunOutcome {
    /// Feedback checks passed for the exact candidate.
    Passed,
    Failed {
        check: String,
    },
    /// A check mutated candidate source; the slot was quarantined.
    Refused {
        reason: String,
    },
    /// A gate failed: the candidate returns to its members. Results remain feedback.
    Blocked {
        reason: String,
    },
    /// Uncommitted; needs explicit retry with fresh checks.
    Interrupted {
        reason: String,
    },
    Published {
        event: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunAttempt {
    pub checks: Vec<Check>,
    /// How the validation slot was obtained: reused, constructed or quarantined.
    pub slot: String,
    pub results: Vec<CheckResult>,
    pub outcome: Option<RunOutcome>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub kind: RunKind,
    pub binding: Binding,
    pub running: bool,
    pub attempts: Vec<RunAttempt>,
    pub outcome: Option<RunOutcome>,
}
/// Host fault/barrier seam for validation; never exposed to untrusted clients.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Prepared,
    Checked(String),
    BeforeAccept,
    /// Inside the acceptance transaction, before commit.
    Committing,
    /// Committed, before the Git mirror and notifications.
    Accepted,
}

pub(crate) fn tables(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS runs (seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, body TEXT NOT NULL);",
    )?;
    Ok(())
}
fn runs(db: &Connection) -> Result<Vec<Run>> {
    let mut stmt = db.prepare("SELECT body FROM runs ORDER BY seq")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
fn run(db: &Connection, id: &str) -> Result<Option<Run>> {
    db.query_row("SELECT body FROM runs WHERE id=?", [id], |r| {
        r.get::<_, String>(0)
    })
    .optional()?
    .map(|b| Ok(serde_json::from_str(&b)?))
    .transpose()
}
fn save_run(db: &Connection, run: &Run) -> Result<()> {
    db.execute(
        "INSERT INTO runs(id, body) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
        params![run.id, serde_json::to_string(run)?],
    )?;
    Ok(())
}
fn published(db: &Connection) -> Result<u64> {
    Ok(meta(db, "published")?
        .ok_or("missing published pointer")?
        .parse()?)
}
fn revision(db: &Connection, revision: u64) -> Result<Capture> {
    let body = db
        .query_row(
            "SELECT body FROM revisions WHERE id=?",
            [revision as i64],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .ok_or("unknown completed revision")?;
    Ok(serde_json::from_str(&body)?)
}
fn required(db: &Connection) -> Result<BTreeSet<usize>> {
    meta(db, "required")?.map_or(Ok(BTreeSet::new()), |r| Ok(serde_json::from_str(&r)?))
}

/// Required offerers and included operations for `revision` over `base`. Every author of
/// an applied operation in (base, revision] counts, even when later edits overwrote it.
pub(crate) fn coverage(
    db: &Connection,
    revision: u64,
    base: u64,
) -> Result<(BTreeSet<usize>, Vec<u64>)> {
    let mut members = required(db)?;
    let mut contributions = Vec::new();
    for record in records(db)? {
        if let Some(Outcome::Applied { revision: r, .. }) = record.outcome
            && base < r
            && r <= revision
        {
            contributions.push(record.operation);
            members.insert(record.agent);
        }
    }
    Ok((members, contributions))
}
fn candidate(db: &Connection, revision_id: u64) -> Result<Candidate> {
    let base = published(db)?;
    let tree = revision(db, revision_id)?.tree;
    let (members, contributions) = coverage(db, revision_id, base)?;
    let mut checkpoints = BTreeMap::new();
    let mut stmt = db.prepare("SELECT body FROM checkpoints ORDER BY rowid")?;
    for body in stmt.query_map([], |r| r.get::<_, String>(0))? {
        let offered: crate::Checkpoint = serde_json::from_str(&body?)?;
        // Readiness never transfers: only an offer bound to this exact candidate counts.
        if offered.state == Availability::Available
            && offered.offer.revision == revision_id
            && offered.base == base
            && offered.members == members
            && offered.contributions == contributions
        {
            checkpoints.insert(offered.agent, offered.id);
        }
    }
    let missing = members
        .iter()
        .filter(|m| !checkpoints.contains_key(m))
        .copied()
        .collect();
    Ok(Candidate {
        binding: Binding {
            revision: revision_id,
            tree,
            base,
            members,
            contributions,
            checkpoints,
        },
        missing,
    })
}
/// Queues the publication run for a completely covered unpublished candidate.
pub(crate) fn queue(db: &Connection, revision: u64) -> Result<()> {
    let candidate = candidate(db, revision)?;
    let binding = candidate.binding;
    if !candidate.missing.is_empty() || revision <= binding.base || binding.members.is_empty() {
        return Ok(());
    }
    let ids: Vec<_> = binding.checkpoints.values().cloned().collect();
    let id = format!("publish:{revision}:{}:{}", binding.base, ids.join(","));
    if run(db, &id)?.is_none() {
        save_run(
            db,
            &Run {
                id,
                kind: RunKind::Publication,
                binding,
                running: false,
                attempts: Vec::new(),
                outcome: None,
            },
        )?;
    }
    Ok(())
}
/// Restart: an uncommitted running attempt is never resumed; it awaits explicit retry.
pub(crate) fn recover(db: &Connection) -> Result<()> {
    for mut run in runs(db)? {
        if run.running {
            let outcome = RunOutcome::Interrupted {
                reason: "restart during validation; explicit retry required".into(),
            };
            run.running = false;
            if let Some(attempt) = run.attempts.last_mut() {
                attempt.outcome = Some(outcome.clone());
            }
            run.outcome = Some(outcome);
            save_run(db, &run)?;
        }
    }
    Ok(())
}
/// The accepted-tip Git ref is only a recoverable mirror of the SQLite pointer.
pub(crate) fn repair_mirror(state: &Path, tree: &str) -> Result<()> {
    const MIRROR: &str = "refs/falinks/published";
    if object(state, &["show-ref", "--verify", "--hash", MIRROR], &[])
        .ok()
        .as_deref()
        != Some(tree)
    {
        object(state, &["update-ref", MIRROR, tree], &[])?;
    }
    Ok(())
}
/// Brings a directory to `target`, rewriting only differing files, then verifies it exactly.
fn converge(slot: &Path, target: &Capture) -> Result<()> {
    for (path, file) in &target.files {
        if read_file(slot, path)? != file.file {
            install(slot, path, &file.file)?;
        }
    }
    audit(slot, target)
}

impl Engine {
    fn slot(&self) -> PathBuf {
        self.state.join("validation")
    }
    /// Host configuration: the fixed required check set. Not durable; results bind to it.
    pub fn configure_checks(&self, checks: Vec<Check>) -> Result<()> {
        if !cfg!(target_os = "macos") {
            return fail("validation checks require verified macOS Seatbelt; unsupported platform");
        }
        let mut names = BTreeSet::new();
        for check in &checks {
            if !names.insert(&check.name)
                || crate::valid_path(&check.name).is_err()
                || !Path::new(&check.program).is_absolute()
            {
                return fail("checks need unique plain names and host executables");
            }
        }
        *self.checks.write().map_err(|_| "checks lock poisoned")? = checks;
        Ok(())
    }
    /// Host configuration: explicitly required group members join every candidate's coverage.
    pub fn require_members(&self, members: BTreeSet<usize>) -> Result<()> {
        if members.iter().any(|m| *m >= 2) {
            return fail("unknown group member");
        }
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let tx = db.unchecked_transaction()?;
        set_meta(&tx, "required", &serde_json::to_string(&members)?)?;
        let mut offered = BTreeSet::new();
        let mut stmt = tx.prepare("SELECT body FROM checkpoints")?;
        for body in stmt.query_map([], |r| r.get::<_, String>(0))? {
            offered.insert(
                serde_json::from_str::<crate::Checkpoint>(&body?)?
                    .offer
                    .revision,
            );
        }
        drop(stmt);
        for revision in offered {
            queue(&tx, revision)?;
        }
        tx.commit()?;
        drop(db);
        self.signal.notify();
        Ok(())
    }
    /// Coverage status of a completed revision against the current published base.
    pub fn candidate(&self, revision: u64) -> Result<Candidate> {
        candidate(&connect(&self.state)?, revision)
    }
    /// Durable lookup, including after a lost response or an uncertain commit.
    pub fn run(&self, id: &str) -> Result<Option<Run>> {
        run(&connect(&self.state)?, id)
    }
    pub fn runs(&self) -> Result<Vec<Run>> {
        runs(&connect(&self.state)?)
    }
    /// Early combined checks of any captured revision, including unfinished team code.
    pub fn feedback(&self, client: &Client, id: &str, revision: u64) -> Result<Run> {
        self.authorize(client)?;
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let id = format!("feedback:{}:{id}", client.agent);
        if let Some(existing) = run(&db, &id)? {
            if existing.binding.revision != revision {
                return fail("feedback request ID reused with changed contents");
            }
            return Ok(existing);
        }
        let capture = captured(&db, revision)?.ok_or("feedback must name a captured revision")?;
        let base = published(&db)?;
        let (members, contributions) = coverage(&db, revision, base)?;
        let created = Run {
            id,
            kind: RunKind::Feedback {
                agent: client.agent,
            },
            binding: Binding {
                revision,
                tree: capture.tree,
                base,
                members,
                contributions,
                checkpoints: BTreeMap::new(),
            },
            running: false,
            attempts: Vec::new(),
            outcome: None,
        };
        save_run(&db, &created)?;
        drop(db);
        self.signal.notify();
        Ok(created)
    }
    /// Explicit retry of an interrupted, uncommitted run; it reruns every gate and check.
    /// Any other recorded outcome is returned unchanged.
    pub fn retry_run(&self, client: &Client, id: &str) -> Result<Run> {
        self.authorize(client)?;
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let mut existing = run(&db, id)?.ok_or("unknown run")?;
        let participant = match existing.kind {
            RunKind::Publication => existing.binding.members.contains(&client.agent),
            RunKind::Feedback { agent } => agent == client.agent,
        };
        if !participant {
            return fail("only a run's members can retry it");
        }
        if matches!(existing.outcome, Some(RunOutcome::Interrupted { .. })) {
            existing.outcome = None;
            save_run(&db, &existing)?;
        }
        Ok(existing)
    }
    /// Host validation worker: processes the oldest queued run, if any.
    pub fn validate(&self) -> Result<Option<Run>> {
        self.validate_observed(|_| Ok(()))
    }
    pub fn validate_observed(
        &self,
        mut observer: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Option<Run>> {
        // The lease covers preparation, checks, acceptance and synchronization.
        let _lease = self
            .validation
            .lock()
            .map_err(|_| "validation lease poisoned")?;
        let checks = self
            .checks
            .read()
            .map_err(|_| "checks lock poisoned")?
            .clone();
        let mut run = {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            let Some(mut run) = runs(&db)?
                .into_iter()
                .find(|r| r.outcome.is_none() && !r.running)
            else {
                return Ok(None);
            };
            run.running = true;
            run.attempts.push(RunAttempt {
                checks: checks.clone(),
                slot: String::new(),
                results: Vec::new(),
                outcome: None,
            });
            save_run(&db, &run)?;
            run
        };
        let outcome = self
            .execute(&mut run, &checks, &mut observer)
            .unwrap_or_else(|error| RunOutcome::Interrupted {
                reason: error.to_string(),
            });
        if !matches!(outcome, RunOutcome::Published { .. }) {
            run.running = false;
            run.attempts.last_mut().unwrap().outcome = Some(outcome.clone());
            run.outcome = Some(outcome.clone());
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            let tx = db.unchecked_transaction()?;
            save_run(&tx, &run)?;
            emit(
                &tx,
                0b11,
                &Body::Validated {
                    run: run.id.clone(),
                    revision: run.binding.revision,
                    tree: run.binding.tree.clone(),
                    outcome,
                },
            )?;
            tx.commit()?;
            drop(db);
            self.signal.notify();
        }
        self.release_slot()?;
        Ok(Some(run))
    }
    fn execute(
        &self,
        run: &mut Run,
        checks: &[Check],
        observer: &mut dyn FnMut(Stage) -> Result<()>,
    ) -> Result<RunOutcome> {
        if checks.is_empty() {
            return Ok(RunOutcome::Blocked {
                reason: "no required checks configured".into(),
            });
        }
        let candidate = revision(&connect(&self.state)?, run.binding.revision)?;
        if run.kind == RunKind::Publication {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            if let Err(reason) = self.recheck(&db, run, checks)? {
                return Ok(RunOutcome::Blocked { reason });
            }
        }
        retain(&self.state, &candidate)?;
        run.attempts.last_mut().unwrap().slot = self.prepare_slot(&run.id, &candidate)?;
        observer(Stage::Prepared)?;
        let slot = self.slot();
        let outputs = self.state.join("validation-runs").join(nonce()?);
        let mut failed = None;
        for check in checks {
            let evidence = outputs.join(&check.name);
            let out = evidence.join("out");
            fs::create_dir_all(&out)?;
            let profile = format!(
                "(version 1) (allow default) (deny network*) (deny file-write*) (allow file-write* (subpath {out}) (literal \"/dev/null\")) (deny file-read-data (subpath {}) (subpath {})) (allow file-read-data (subpath {}) (subpath {out}))",
                quote(&self.state)?,
                quote(&self.live)?,
                quote(&slot)?,
                out = quote(&out)?,
            );
            let status = sandboxed(
                &evidence,
                &profile,
                &check.program,
                &check.args,
                &slot,
                &[
                    ("TMPDIR", &out),
                    ("HOME", &out),
                    ("CARGO_TARGET_DIR", &out.join("target")),
                    ("GOCACHE", &out.join("go-cache")),
                    ("GOPATH", &out.join("go")),
                    ("GOTOOLCHAIN", Path::new("local")),
                    ("GOPROXY", Path::new("off")),
                ],
                Duration::from_secs(300),
            );
            let passed = status.as_ref().is_ok_and(|s| s.success());
            if !passed && failed.is_none() {
                failed = Some(check.name.clone());
            }
            run.attempts.last_mut().unwrap().results.push(CheckResult {
                name: check.name.clone(),
                passed,
                status: match status {
                    Ok(status) => status.to_string(),
                    Err(error) => error.to_string(),
                },
                stdout: fs::read(evidence.join("stdout")).unwrap_or_default(),
                stderr: fs::read(evidence.join("stderr")).unwrap_or_default(),
            });
            observer(Stage::Checked(check.name.clone()))?;
            // Candidate source must stay fixed; a mutating check is refused, never trusted.
            if let Err(error) = audit(&slot, &candidate) {
                let note = self.quarantine()?;
                return Ok(RunOutcome::Refused {
                    reason: format!(
                        "check {} mutated candidate source: {error}; {note}",
                        check.name
                    ),
                });
            }
        }
        // Logs are retained in the run record; build output never reaches another run.
        let _ = fs::remove_dir_all(&outputs);
        if let Some(check) = failed {
            return Ok(RunOutcome::Failed { check });
        }
        match run.kind {
            RunKind::Feedback { .. } => Ok(RunOutcome::Passed),
            RunKind::Publication => self.accept(run, checks, &candidate, observer),
        }
    }
    /// Every acceptance gate, rechecked under the writer lock.
    fn recheck(
        &self,
        db: &Connection,
        run: &Run,
        checks: &[Check],
    ) -> Result<std::result::Result<(), String>> {
        let binding = &run.binding;
        let base = published(db)?;
        if base != binding.base {
            return Ok(Err(if binding.revision <= base {
                "a newer publication was accepted and is never overwritten".into()
            } else {
                "published base advanced; reconsider and offer against the new base".into()
            }));
        }
        let current = candidate(db, binding.revision)?;
        if !current.missing.is_empty() || current.binding != *binding {
            return Ok(Err(format!(
                "offer coverage or bindings changed; missing offers from {:?}",
                current.missing
            )));
        }
        for agent in &binding.members {
            if obligations(db, *agent)?
                .iter()
                .any(|o| o.pending() && o.required <= binding.revision)
            {
                return Ok(Err(format!(
                    "agent {agent} has an unreviewed included change"
                )));
            }
        }
        if let Some(reason) = meta(db, "halted")? {
            return Ok(Err(format!("engine halted: {reason}")));
        }
        if *self.checks.read().map_err(|_| "checks lock poisoned")? != checks {
            return Ok(Err("required check set changed".into()));
        }
        Ok(Ok(()))
    }
    fn accept(
        &self,
        run: &mut Run,
        checks: &[Check],
        candidate: &Capture,
        observer: &mut dyn FnMut(Stage) -> Result<()>,
    ) -> Result<RunOutcome> {
        observer(Stage::BeforeAccept)?;
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        if let Err(reason) = self.recheck(&db, run, checks)? {
            return Ok(RunOutcome::Blocked { reason });
        }
        let completed = self
            .completed
            .read()
            .map_err(|_| "completed lock poisoned")?
            .clone();
        if let Err(error) = audit(&self.live, &completed) {
            let reason = error.to_string();
            self.save_incident(&db, &reason)?;
            return Ok(RunOutcome::Blocked {
                reason: format!("unexplained source change stops publication: {reason}"),
            });
        }
        retain(&self.state, candidate)?;
        let binding = &run.binding;
        // One commit: pointer, every member status, run outcome and the ordered event.
        let tx = db.unchecked_transaction()?;
        set_meta(&tx, "published", &binding.revision.to_string())?;
        let event = emit(
            &tx,
            0b11,
            &Body::Published {
                run: run.id.clone(),
                revision: binding.revision,
                base: binding.base,
                tree: binding.tree.clone(),
                checkpoints: binding.checkpoints.clone(),
            },
        )?;
        for id in binding.checkpoints.values() {
            let mut accepted = checkpoint(&tx, id)?.ok_or("covering checkpoint missing")?;
            accepted.state = Availability::Accepted { event };
            save_checkpoint(&tx, &accepted)?;
        }
        let outcome = RunOutcome::Published { event };
        run.running = false;
        run.attempts.last_mut().unwrap().outcome = Some(outcome.clone());
        run.outcome = Some(outcome.clone());
        save_run(&tx, run)?;
        observer(Stage::Committing)?;
        tx.commit()?;
        drop(db);
        observer(Stage::Accepted)?;
        // A failed mirror update never undoes acceptance; startup repairs it from SQLite.
        let _ = repair_mirror(&self.state, &candidate.tree);
        self.signal.notify();
        Ok(outcome)
    }
    /// A free lease after a crash proves nothing: reuse only a recorded clean slot that
    /// still verifies; otherwise quarantine it and reconstruct.
    fn prepare_slot(&self, run: &str, candidate: &Capture) -> Result<String> {
        let previous = {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            let previous = meta(&db, "slot")?;
            set_meta(&db, "slot", &format!("dirty:{run}"))?;
            previous
        };
        let clean = previous
            .and_then(|p| p.strip_prefix("clean:").map(str::to_owned))
            .and_then(|r| r.parse().ok())
            .map(|r| revision(&connect(&self.state)?, r))
            .transpose()?;
        let slot = self.slot();
        let note = match clean {
            Some(known) if audit(&slot, &known).is_ok() => "reused".to_string(),
            _ => {
                let note = self.quarantine()?;
                fs::create_dir_all(&slot)?;
                note
            }
        };
        converge(&slot, candidate)?;
        Ok(note)
    }
    fn quarantine(&self) -> Result<String> {
        let slot = self.slot();
        if !slot.exists() {
            return Ok("constructed".into());
        }
        let quarantine = self.state.join("quarantine");
        fs::create_dir_all(&quarantine)?;
        let target = quarantine.join(nonce()?);
        fs::rename(&slot, &target)?;
        Ok(format!("quarantined {}", target.display()))
    }
    /// Synchronize to the SQLite-authoritative published snapshot before releasing.
    fn release_slot(&self) -> Result<()> {
        let published = self.published()?;
        let slot = self.slot();
        if slot.exists() && converge(&slot, &published).is_ok() {
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            set_meta(&db, "slot", &format!("clean:{}", published.revision))?;
        } else {
            // Recorded dirty first: a crash mid-quarantine still forces reconstruction.
            let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
            db.execute("DELETE FROM meta WHERE key='slot'", [])?;
            drop(db);
            self.quarantine()?;
        }
        Ok(())
    }
}
