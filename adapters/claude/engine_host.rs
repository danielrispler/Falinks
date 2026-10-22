//! The production `falinks::Engine` behind authenticated tool envelopes (#26).
//! The boundary has already bound host identity; this maps each operation to the
//! engine as that agent's opaque `Client`. Agent mistakes are refusals; an identity
//! mismatch or an engine incident is an error that latches the worker's gate.
use falinks::{
    Body, Capture, Checkpoint, Client, Engine, Event, File, Hunk, JobSpec, Mode, Outcome, Output,
    Pending, Request, Review, valid_path,
};
use falinks_host::{Result, require, tool};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs,
    io::Read,
    os::{
        fd::{FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct EngineHost {
    engine: Arc<Engine>,
    client: Client,
    agent: String,
    /// The one host-enrolled job program (`gofmt -w`); agents name paths only.
    formatter: PathBuf,
    /// The agent's own scratch, where `{from}` references resolve.
    scratch: PathBuf,
}
impl EngineHost {
    pub fn new(
        engine: Arc<Engine>,
        client: Client,
        agent: &str,
        formatter: PathBuf,
        scratch: PathBuf,
    ) -> Self {
        Self {
            engine,
            client,
            agent: agent.into(),
            formatter,
            scratch,
        }
    }
    pub fn call(&self, envelope: &Value) -> Result<Value> {
        Ok(self.call_timed(envelope)?.0)
    }
    /// `call`, plus an edit's engine stage report (host-only) when the call reached the engine.
    pub fn call_timed(&self, envelope: &Value) -> Result<(Value, Option<Value>)> {
        let identity = &envelope["identity"];
        let root = self.engine.root(&self.client)?;
        require(
            identity["agent"] == self.agent.as_str()
                && identity["workspace"].as_str() == root.to_str(),
            "envelope identity is not this agent's current workspace",
        )?;
        let (operation, request) = (
            envelope["operation"].as_str().unwrap_or(""),
            &envelope["request"],
        );
        let mut stages = None;
        let outcome = self.dispatch(operation, request, &mut stages);
        // An unknown change halts the engine: stop this worker's capability too.
        if let Some(incident) = self.engine.incident()? {
            return Err(format!("engine incident: {}", incident.reason).into());
        }
        let result = match outcome {
            Ok(value) => text(lean(value)),
            Err(error) => json!({"accepted":false,"reason":error.to_string()}),
        };
        Ok((result, stages))
    }
    /// Each operation's parser checks its request keys before the engine sees the request.
    fn dispatch(
        &self,
        operation: &str,
        request: &Value,
        stages: &mut Option<Value>,
    ) -> Result<Value> {
        let (engine, client) = (&self.engine, &self.client);
        let takes = |required: &[&str], optional: &[&str]| {
            require_keys(request, operation, required, optional)
        };
        Ok(match operation {
            "capture" => {
                takes(&[], &["paths"])?;
                let capture = engine.capture_for(client)?;
                let wanted: Vec<&str> = request["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let mut reply = view(&capture, |path| wanted.contains(&path));
                reply["accepted"] = json!(true);
                reply["space"] = json!(capture.space);
                reply
            }
            "edit" | "retry" | "incorporate" => {
                takes(&["id", "expected", "output", "note"], &["reviews"])?;
                require_review_keys(request)?;
                let request = edit(request, &self.scratch)?;
                let mode = match operation {
                    "edit" => Mode::Apply,
                    "retry" => Mode::Retry,
                    _ => Mode::Incorporate,
                };
                let (outcome, report) = engine.apply_timed(client, request, mode);
                *stages = Some(json!(report
                    .iter()
                    .map(|(stage, took)| json!({"stage": format!("{stage:?}"), "seconds": took.as_secs_f64()}))
                    .collect::<Vec<_>>()));
                let outcome = outcome?;
                json!({"accepted":matches!(outcome, Outcome::Applied{..}),"outcome":outcome})
            }
            "request" => {
                takes(&["id"], &[])?;
                let record = engine.request(client, field(request, "id")?)?;
                json!({"accepted":record.is_some(),"record":record})
            }
            "register" => {
                takes(&["id", "task", "nodes"], &["depends"])?;
                json!({"accepted":true,"obligation":engine.register(client, parse(request)?)?})
            }
            "obligations" => {
                takes(&[], &[])?;
                json!({"accepted":true,"obligations":engine.obligations(client)?})
            }
            "review" => {
                takes(REVIEW, &[])?;
                json!({"accepted":true,"obligation":engine.review(client, parse(request)?)?})
            }
            "offer" => {
                takes(
                    &["id", "task", "revision", "scope", "text"],
                    &["supersedes", "reviews"],
                )?;
                require_review_keys(request)?;
                let offered = engine.offer(client, parse(request)?)?;
                let (mut reply, coverage) = (state(&offered.checkpoint), offered.coverage);
                reply["missing"] = json!(coverage.missing);
                reply["queued"] = json!(coverage.queued);
                reply["reason"] = json!(coverage.reason);
                reply
            }
            "handle" => {
                takes(&[], &["through", "deferred", "processed"])?;
                let deferred: Option<BTreeMap<u64, String>> = parse(&request["deferred"])?;
                let processed: Option<Vec<u64>> = parse(&request["processed"])?;
                engine.handle(
                    client,
                    parse(&request["through"])?,
                    &deferred.unwrap_or_default(),
                    &processed.unwrap_or_default(),
                )?;
                json!({"accepted":true})
            }
            "post" => {
                takes(&["task", "scope", "work", "text"], &["reply_to"])?;
                let reply_to = &request["reply_to"];
                if !reply_to.is_null() {
                    require_keys(reply_to, "reply_to", &["event", "work"], &[])?;
                }
                json!({"accepted":true,"seq":engine.post(client, parse(request)?)?.seq})
            }
            "withdraw" => {
                takes(&["offer"], &[])?;
                state(&engine.withdraw(client, field(request, "offer")?)?)
            }
            "checkpoint" => {
                takes(&["id"], &[])?;
                match engine.checkpoint(field(request, "id")?)? {
                    Some(checkpoint) => state(&checkpoint),
                    None => json!({"accepted":false}),
                }
            }
            "candidate" => {
                takes(&["revision"], &[])?;
                let candidate = engine.candidate(number(request, "revision")?)?;
                json!({"accepted":true,"binding":candidate.binding,"missing":candidate.missing})
            }
            "feedback" => {
                takes(&["id", "revision"], &[])?;
                let run =
                    engine.feedback(client, field(request, "id")?, number(request, "revision")?)?;
                json!({"accepted":true,"run":run})
            }
            "run" => {
                takes(&["id"], &[])?;
                let run = engine.run(field(request, "id")?)?;
                json!({"accepted":run.is_some(),"run":run})
            }
            "recommend" => {
                takes(&[], &[])?;
                json!({"accepted":true,"proposal":engine.recommend(client)?})
            }
            "propose" => {
                takes(&["action", "text", "failing"], &[])?;
                json!({"accepted":true,"proposal":engine.propose(client, parse(request)?)?})
            }
            "respond" => {
                takes(&["id", "context", "response"], &[])?;
                // `"Agree"` or `{Decline: {reason}}`; serde ignores unknown keys in the latter.
                let response = &request["response"];
                if response.is_object() {
                    require_keys(response, "response", &["Decline"], &[])?;
                    require_keys(&response["Decline"], "response.Decline", &["reason"], &[])?;
                }
                let proposal = engine.respond(
                    client,
                    field(request, "id")?,
                    field(request, "context")?,
                    parse(response)?,
                )?;
                json!({"accepted":true,"proposal":proposal})
            }
            "job" => {
                takes(&["id", "revision", "paths"], &[])?;
                let paths: Vec<String> = parse(&request["paths"])?;
                // gofmt creates `$HOME/Library` (undeclared output) unless HOME is unwritable.
                let mut args = ["HOME=/var/empty", &self.formatter.to_string_lossy(), "-w"]
                    .map(String::from)
                    .to_vec();
                args.extend(paths.iter().cloned());
                let job = engine.run_job(
                    client,
                    JobSpec {
                        id: field(request, "id")?.into(),
                        revision: number(request, "revision")?,
                        inputs: paths.clone(),
                        outputs: paths,
                        program: "/usr/bin/env".into(),
                        args,
                    },
                )?;
                json!({"accepted":job.error.is_none(),"job":job})
            }
            "apply_job" => {
                takes(&["id"], &[])?;
                let outcome = engine.apply_job(client, field(request, "id")?)?;
                json!({"accepted":matches!(outcome, Outcome::Applied{..}),"outcome":outcome})
            }
            _ => return Err(format!("unsupported operation {operation}").into()),
        })
    }
}

/// The JSON type of each key of an edit, retry or incorporation.
const EDIT_TYPES: &str =
    r#"{"id":"string","expected":"object","output":"object","note":"string","reviews":"array"}"#;
/// Agent-facing operations, the JSON type of each request key, and their request shapes.
/// Host-only calls (checks, validation, workspaces, reconsideration, disconnect, run retry,
/// space-0 capture, `pending`, `events`) are never tools.
pub const OPERATIONS: &[(&str, &str, &str)] = &[
    (
        "capture",
        r#"{"paths":"array"}"#,
        "{paths?: [path]} -> your workspace's last completed revision, every file's version, and the text of the named paths. Native Read can show a partial installation; capture never does.",
    ),
    (
        "edit",
        EDIT_TYPES,
        "{id, expected: {path: version}, output: {path: {content, executable?} | {replace: [{old, new}], executable?} | {from, executable?} | null}, note, reviews?: [{scope, revision, decision: Keep|Revise|Drop, note}]} -> applies all outputs or nothing. Each output takes exactly one form. content is the whole new file. replace edits an existing file: each old is non-empty and matches the expected version's text exactly once, hunks do not overlap, and all apply together. replace keeps the file's mode unless executable is given. from is a path relative to your own scratch directory, read as the whole new file: a regular file, not a symlink, hard link or under a symlinked directory, at most 1 MiB of UTF-8 text, with no hidden or .. parts. executable comes only from the request, never the scratch file's mode. A refused from reference uses no id. Do not JSON-encode files for edit. note is required and non-empty: a short explanation your peer sees as the live edit's text, so do not post it again. expected names every input and output. Stale returns current versions; reread and use a new id. Unreviewed names the obligations that need a review. reviews are review requests sent inline: each is checked first, any invalid one refuses the whole call, and valid ones are recorded before the edit is gated, so they stand even if it comes back Stale.",
    ),
    (
        "retry",
        EDIT_TYPES,
        "Same shape as edit, same id and note: explicitly retry an interrupted request after restart.",
    ),
    (
        "incorporate",
        EDIT_TYPES,
        "Same shape as edit, note included: install the published state with your resolution of every file a Behind event names.",
    ),
    (
        "request",
        r#"{"id":"string"}"#,
        "{id} -> your retained request, attempts and outcome.",
    ),
    (
        "register",
        r#"{"id":"string","task":"string","nodes":"array","depends":"array"}"#,
        "{id, task, nodes: [path | path#Symbol], depends?: [node]} -> declare intended work before editing.",
    ),
    (
        "obligations",
        "{}",
        "{} -> your registered scopes with required and reviewed revisions.",
    ),
    (
        "review",
        r#"{"scope":"string","revision":"integer","decision":"string","note":"string"}"#,
        "{scope, revision, decision: Keep|Revise|Drop, note} -> after rereading a revision that was captured for you: by your capture call, or the capture in a host notice. The only way to clear an obligation; edit and offer take the same reviews inline.",
    ),
    (
        "post",
        r#"{"task":"string","scope":"array","work":["string","object"],"text":"string","reply_to":["object","null"]}"#,
        "{task, scope: [scope id], work: \"None\" | {Revision: n} | {Checkpoint: id}, text, reply_to?: null | {event: seq, work}} -> advisory plan message to your peer; returns its seq. Only messages can be replied to: plans, live edits and checkpoint offers (Message events), never Reviewed, Obligation or other events. reply_to.work is the original message's work.",
    ),
    (
        "offer",
        r#"{"id":"string","task":"string","revision":"integer","scope":"array","text":"string","supersedes":"string","reviews":"array"}"#,
        "{id, task, revision, scope: [scope id], text, supersedes?: offer id, reviews?: [{scope, revision, decision, note}]} -> offer an exact captured revision for publication. Refused while any obligation is pending. reviews are checked and recorded first, as in edit. The result reports coverage as of this offer: missing names the members still lacking an offer of this revision, queued says whether a publication run was queued, and reason (missing-members, not-ahead-of-base or no-members) says why not. The outcome arrives later as a Published or Validated event.",
    ),
    (
        "withdraw",
        r#"{"offer":"string"}"#,
        "{offer} -> withdraw your offer.",
    ),
    (
        "checkpoint",
        r#"{"id":"string"}"#,
        "{id: \"agent/offer-id\"} -> an offer's state (Available, Withdrawn, Superseded, Accepted).",
    ),
    (
        "candidate",
        r#"{"revision":"integer"}"#,
        "{revision} -> the exact candidate binding and members still missing an offer.",
    ),
    (
        "feedback",
        r#"{"id":"string","revision":"integer"}"#,
        "{id, revision} -> queue required checks on a captured revision early; never publishes.",
    ),
    (
        "run",
        r#"{"id":"string"}"#,
        "{id} -> a check run with its results and outcome.",
    ),
    (
        "handle",
        r#"{"through":"integer","deferred":"object","processed":"array"}"#,
        "{through?: seq, deferred?: {seq: note}, processed?: [seq]} -> one atomic call. through handles every event in your stream after your cursor, up to that seq, as processed unless deferred lists it, and moves your cursor there; at or below the cursor it does nothing. It may be your own post's seq. The engine does not track what you were shown, so through also covers events no notice gave you. deferred keeps a note for an event at or below the new cursor; processed clears earlier deferrals. Never clears obligations.",
    ),
    (
        "recommend",
        "{}",
        "{} -> ask the engine whether to join, split or keep the current arrangement.",
    ),
    (
        "propose",
        r#"{"action":"string","text":"string","failing":"boolean"}"#,
        "{action: Join|Split|Keep, text, failing: bool} -> propose a regrouping with your reasons.",
    ),
    (
        "respond",
        r#"{"id":"string","context":"string","response":["string","object"]}"#,
        "{id, context, response: \"Agree\" | {Decline: {reason}}} -> answer a regrouping proposal; repeat its exact context.",
    ),
    (
        "job",
        r#"{"id":"string","revision":"integer","paths":"array"}"#,
        "{id, revision, paths: [path]} -> run the host-enrolled gofmt over captured paths in isolation.",
    ),
    (
        "apply_job",
        r#"{"id":"string"}"#,
        "{id} -> apply a finished job's output through the normal freshness gate.",
    ),
];
/// MCP definitions for every agent-facing operation; each takes its request as its input.
pub fn tools() -> Value {
    json!(
        OPERATIONS
            .iter()
            .map(|(operation, types, help)| tool(
                operation,
                types,
                format!("Falinks {operation}: {help}")
            ))
            .collect::<Vec<_>>()
    )
}

/// A capture as agents see it: the revision, every file's version, and the text of the
/// files `content` selects.
fn view(capture: &Capture, content: impl Fn(&str) -> bool) -> Value {
    let files: serde_json::Map<String, Value> = capture
        .files
        .iter()
        .map(|(path, versioned)| {
            let mut entry = json!({"version":versioned.version,"exists":versioned.file.is_some()});
            if let Some(file) = &versioned.file
                && content(path)
            {
                entry["content"] = json!(String::from_utf8_lossy(&file.bytes));
                entry["executable"] = json!(file.executable);
            }
            (path.clone(), entry)
        })
        .collect();
    json!({"revision":capture.revision,"files":files})
}

/// The opening protocol state from a capture given to the agent: the revision, file
/// versions and pending state. No file contents, so both arms get the same task
/// information (#5). Also returns the file versions given.
pub fn opening(engine: &Engine, client: &Client) -> Result<(Value, BTreeMap<String, u64>)> {
    let capture = engine.capture_for(client)?;
    let pending = engine.pending(client)?;
    let mut state = view(&capture, |_| false);
    state["pending"] = json!({"handled":pending.handled,"unhandled":pending.unhandled,
        "deferred":deferred(&pending),"obligations":pending.obligations});
    Ok((state, versions(&capture)))
}

/// When `events` renewed the client's obligation, a capture given to it with the text of
/// every file whose version differs from `given`, which it then updates. The capture is
/// recorded at send, not at presentation (#116 decision 3).
pub fn notice_capture(
    engine: &Engine,
    client: &Client,
    events: &[Event],
    given: &mut BTreeMap<String, u64>,
) -> Result<Option<Value>> {
    // Obligation events go only to the agent whose obligation they renew.
    if !events
        .iter()
        .any(|e| matches!(e.body, Body::Obligation { .. }))
    {
        return Ok(None);
    }
    let capture = engine.capture_for(client)?;
    let state = view(&capture, |path| {
        given.get(path) != capture.files.get(path).map(|f| &f.version)
    });
    *given = versions(&capture);
    Ok(Some(state))
}

fn versions(capture: &Capture) -> BTreeMap<String, u64> {
    capture
        .files
        .iter()
        .map(|(path, f)| (path.clone(), f.version))
        .collect()
}

/// Notice events as agents see them: each event's ID is its `seq`, and a `Message` drops
/// `workspace`, which the notice header names once (#221).
pub fn notice_events(events: &[Event]) -> Result<Value> {
    let mut shown = serde_json::to_value(events)?;
    for event in shown.as_array_mut().into_iter().flatten() {
        if let Some(event) = event.as_object_mut() {
            event.remove("id");
        }
        if let Some(message) = event["body"]["Message"].as_object_mut() {
            message.remove("workspace");
        }
    }
    Ok(shown)
}
/// The notice header's ID: the last event's seq and the workspace identity, the
/// nonce in the engine's `nonce:seq` event IDs.
pub fn notice_header(events: &[Event]) -> Option<String> {
    let last = events.last()?;
    let nonce = last.id.split_once(':').map_or(last.id.as_str(), |(n, _)| n);
    Some(format!("{} (workspace {nonce})", last.seq))
}

/// Deferred events as agents see them: `{event, note}`.
pub fn deferred(pending: &Pending) -> Vec<Value> {
    pending
        .deferred
        .iter()
        .map(|(event, note)| json!({"event":event,"note":note}))
        .collect()
}

/// The host worker that processes queued check runs while agents keep drafting, until
/// `stop`; `pause` holds runs queued. Returns its log.
pub fn validator(
    engine: Arc<Engine>,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
) -> JoinHandle<Vec<Value>> {
    thread::spawn(move || {
        let mut log = vec![];
        while !stop.load(Ordering::SeqCst) {
            match (!pause.load(Ordering::SeqCst)).then(|| engine.validate()) {
                Some(Ok(Some(run))) => log.push(json!({"run":run.id,"outcome":run.outcome})),
                Some(Ok(None)) | None => thread::sleep(Duration::from_millis(200)),
                Some(Err(error)) => {
                    log.push(json!({"error":error.to_string()}));
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
        log
    })
}

/// Refuses `value` unless it is an object with every `required` key and no key beyond
/// `required` and `optional`. Parsers run it before the engine sees a request, so a refusal
/// uses up no request ID.
pub(crate) fn require_keys(
    value: &Value,
    what: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<()> {
    let given: Vec<&str> = value
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys().map(String::as_str))
        .collect();
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|k| !given.contains(k))
        .collect();
    let unknown: Vec<&str> = given
        .iter()
        .copied()
        .filter(|k| !required.contains(k) && !optional.contains(k))
        .collect();
    if value.is_object() && missing.is_empty() && unknown.is_empty() {
        return Ok(());
    }
    let expected: Vec<String> = required
        .iter()
        .map(|k| k.to_string())
        .chain(optional.iter().map(|k| format!("{k}?")))
        .collect();
    Err(format!(
        "request keys: {what} takes {{{}}}; missing {missing:?}, unknown {unknown:?}",
        expected.join(", ")
    )
    .into())
}
/// The keys of a review: the `review` request and each inline review.
const REVIEW: &[&str] = &["scope", "revision", "decision", "note"];
fn require_review_keys(request: &Value) -> Result<()> {
    for review in request["reviews"].as_array().into_iter().flatten() {
        require_keys(review, "reviews[]", REVIEW, &[])?;
    }
    Ok(())
}
fn parse<T: serde::de::DeserializeOwned>(request: &Value) -> Result<T> {
    Ok(serde_json::from_value(request.clone())?)
}
fn number(request: &Value, name: &str) -> Result<u64> {
    Ok(request[name]
        .as_u64()
        .ok_or(format!("request.{name} must be a number"))?)
}
fn field<'a>(request: &'a Value, name: &str) -> Result<&'a str> {
    Ok(request[name]
        .as_str()
        .ok_or(format!("request.{name} must be a string"))?)
}
/// `{id, expected: {path: version}, output: {path: {content | replace | from, executable?} | null},
/// note, reviews?}`. A `from` reference is resolved here, so a refused one uses up no request ID.
fn edit(request: &Value, scratch: &Path) -> Result<Request> {
    let expected: BTreeMap<String, u64> = serde_json::from_value(request["expected"].clone())?;
    let mut output = BTreeMap::new();
    for (path, file) in request["output"]
        .as_object()
        .ok_or("request.output must map paths to {content}, {replace}, {from} or null")?
    {
        let file = match file {
            Value::Null => Output::Delete,
            file => {
                let what = format!("output {path:?}");
                let forms: Vec<&str> = FORMS
                    .into_iter()
                    .filter(|form| file.get(form).is_some())
                    .collect();
                let [form] = forms[..] else {
                    return Err(format!("request keys: {what} has {forms:?}; {ONE_FORM}").into());
                };
                require_keys(file, &what, &[form], &["executable"])
                    .map_err(|e| format!("{e}; {ONE_FORM}"))?;
                let executable = file["executable"].as_bool();
                let text = |key: &str| -> Result<Vec<u8>> {
                    Ok(file[key]
                        .as_str()
                        .ok_or(format!("{what}.{key} must be text"))?
                        .as_bytes()
                        .to_vec())
                };
                match form {
                    "content" => Output::Whole(File {
                        bytes: text("content")?,
                        executable: executable.unwrap_or(false),
                    }),
                    "replace" => Output::Replace {
                        replace: file["replace"]
                            .as_array()
                            .ok_or(format!("{what}.replace must be an array"))?
                            .iter()
                            .map(|hunk| {
                                require_keys(hunk, "replace[]", &["old", "new"], &[])?;
                                let text = |key: &str| -> Result<Vec<u8>> {
                                    Ok(hunk[key]
                                        .as_str()
                                        .ok_or(format!("replace[].{key} must be text"))?
                                        .as_bytes()
                                        .to_vec())
                                };
                                Ok(Hunk {
                                    old: text("old")?,
                                    new: text("new")?,
                                })
                            })
                            .collect::<Result<_>>()?,
                        executable,
                    },
                    _ => Output::Whole(File {
                        bytes: scratch_file(scratch, &String::from_utf8(text("from")?)?)
                            .map_err(|e| format!("{what}.from refused: {e}"))?,
                        // Never the scratch file's mode.
                        executable: executable.unwrap_or(false),
                    }),
                }
            }
        };
        output.insert(path.clone(), file);
    }
    let reviews: Option<Vec<Review>> = parse(&request["reviews"])?;
    let id = field(request, "id")?;
    Ok(Request {
        id: id.into(),
        expected,
        output,
        note: field(request, "note")?.into(),
        reviews: reviews.unwrap_or_default(),
    })
}
/// The edit output forms; a value takes exactly one.
const FORMS: [&str; 3] = ["content", "replace", "from"];
const ONE_FORM: &str = "an output takes exactly one of {content, executable?}, \
    {replace: [{old, new}], executable?} or {from, executable?}";
/// The largest file a scratch reference may name.
const SCRATCH_LIMIT: u64 = 1 << 20;
/// Reads `path` beneath `scratch` for a `{from}` reference. The host runs unsandboxed, so a
/// link in scratch must not let it copy protected files into the live workspace (#193): each
/// component opens without following symlinks, and every check runs on the opened file.
fn scratch_file(scratch: &Path, path: &str) -> Result<Vec<u8>> {
    valid_path(path).map_err(|e| format!("{e} (a path relative to your scratch)"))?;
    let open = |dir: Option<&OwnedFd>, name: &[u8], flags: libc::c_int| -> Result<OwnedFd> {
        let name = CString::new(name)?;
        let flags = flags | libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let at = dir.map_or(libc::AT_FDCWD, std::os::fd::AsRawFd::as_raw_fd);
        // SAFETY: `name` is a valid C string and `at` is open or AT_FDCWD.
        let fd = unsafe { libc::openat(at, name.as_ptr(), flags) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            return Err(match error.raw_os_error() {
                Some(libc::ELOOP | libc::ENOTDIR | libc::EMLINK) => {
                    format!("symlink or non-directory in {path:?}").into()
                }
                _ => format!("cannot open {path:?}: {error}").into(),
            });
        }
        // SAFETY: `fd` was just opened and is owned by nothing else.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    };
    let mut dir = open(None, scratch.as_os_str().as_bytes(), libc::O_DIRECTORY)?;
    let parts: Vec<&str> = path.split('/').collect();
    let (name, parents) = parts.split_last().ok_or("empty path")?;
    for part in parents {
        dir = open(Some(&dir), part.as_bytes(), libc::O_DIRECTORY)?;
    }
    // O_NONBLOCK: opening a FIFO must not wait for a writer.
    let mut file = fs::File::from(open(Some(&dir), name.as_bytes(), libc::O_NONBLOCK)?);
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(format!("{path:?} is not a regular file").into());
    }
    if metadata.nlink() != 1 {
        return Err(format!("{path:?} is a hard link").into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(SCRATCH_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > SCRATCH_LIMIT {
        return Err(format!("{path:?} is over 1 MiB").into());
    }
    String::from_utf8(bytes)
        .map_err(|_| format!("{path:?} is not valid UTF-8").into())
        .map(String::into_bytes)
}
/// Results drop the engine's Git tree and blob IDs, which agents never use (#221). Only
/// string or null values go, so a file named `tree` or `blob` stays.
/// An offer's state without its capture: agents already hold the content (#222).
fn state(checkpoint: &Checkpoint) -> Value {
    json!({"accepted":true,"id":checkpoint.id,"state":checkpoint.state,
        "revision":checkpoint.offer.revision,"scope":checkpoint.offer.scope})
}
fn lean(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, value)| {
                    !(matches!(key.as_str(), "tree" | "blob")
                        && (value.is_string() || value.is_null()))
                })
                .map(|(key, value)| (key, lean(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(lean).collect()),
        other => other,
    }
}
/// Agents read and write text: engine byte arrays become `content` strings.
fn text(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| match (key.as_str(), &value) {
                    ("bytes", Value::Array(_)) => ("content".into(), lossy(&value)),
                    ("stdout" | "stderr", Value::Array(_)) => (key, lossy(&value)),
                    _ => (key, text(value)),
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(text).collect()),
        other => other,
    }
}
fn lossy(bytes: &Value) -> Value {
    let bytes: Vec<u8> = serde_json::from_value(bytes.clone()).unwrap_or_default();
    json!(String::from_utf8_lossy(&bytes))
}
