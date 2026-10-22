//! The production `falinks::Engine` behind authenticated tool envelopes (#26).
//! The boundary has already bound host identity; this maps each operation to the
//! engine as that agent's opaque `Client`. Agent mistakes are refusals; an identity
//! mismatch or an engine incident is an error that latches the worker's gate.
use falinks::{
    Body, Capture, Client, Engine, Event, File, JobSpec, Mode, Outcome, Output, Pending, Request,
    Review,
};
use falinks_host::{Result, require, tool};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
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
}
impl EngineHost {
    pub fn new(engine: Arc<Engine>, client: Client, agent: &str, formatter: PathBuf) -> Self {
        Self {
            engine,
            client,
            agent: agent.into(),
            formatter,
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
        let request = &envelope["request"];
        let mut stages = None;
        let outcome = self.dispatch(
            envelope["operation"].as_str().unwrap_or(""),
            request,
            &mut stages,
        );
        // An unknown change halts the engine: stop this worker's capability too.
        if let Some(incident) = self.engine.incident()? {
            return Err(format!("engine incident: {}", incident.reason).into());
        }
        let result = match outcome {
            Ok(value) => text(value),
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
                reply["tree"] = json!(capture.tree);
                reply["space"] = json!(capture.space);
                reply
            }
            "edit" | "retry" | "incorporate" => {
                takes(&["id", "expected", "output"], &["reviews"])?;
                require_review_keys(request)?;
                let request = edit(request)?;
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
                let coverage = offered.coverage;
                json!({"accepted":true,"checkpoint":offered.checkpoint,"missing":coverage.missing,
                    "queued":coverage.queued,"reason":coverage.reason})
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
                json!({"accepted":true,"event":engine.post(client, parse(request)?)?})
            }
            "withdraw" => {
                takes(&["offer"], &[])?;
                json!({"accepted":true,"checkpoint":engine.withdraw(client, field(request, "offer")?)?})
            }
            "checkpoint" => {
                takes(&["id"], &[])?;
                let checkpoint = engine.checkpoint(field(request, "id")?)?;
                json!({"accepted":checkpoint.is_some(),"checkpoint":checkpoint})
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

/// Agent-facing operations and their request shapes. Host-only calls (checks, validation,
/// workspaces, reconsideration, disconnect, run retry, space-0 capture, `pending`, `events`)
/// are never tools.
pub const OPERATIONS: &[(&str, &str)] = &[
    (
        "capture",
        "{paths?: [path]} -> your workspace's last completed revision, every file's version, and the text of the named paths. Native Read can show a partial installation; capture never does.",
    ),
    (
        "edit",
        "{id, expected: {path: version}, output: {path: {content, executable?} | null}, reviews?: [{scope, revision, decision: Keep|Revise|Drop, note}]} -> applies all outputs or nothing. expected names every input and output. Stale returns current versions; reread and use a new id. Unreviewed names the obligations that need a review. reviews are review requests sent inline: each is checked first, any invalid one refuses the whole call, and valid ones are recorded before the edit is gated, so they stand even if it comes back Stale.",
    ),
    (
        "retry",
        "Same shape as edit, same id: explicitly retry an interrupted request after restart.",
    ),
    (
        "incorporate",
        "Same shape as edit: install the published state with your resolution of every file a Behind event names.",
    ),
    (
        "request",
        "{id} -> your retained request, attempts and outcome.",
    ),
    (
        "register",
        "{id, task, nodes: [path | path#Symbol], depends?: [node]} -> declare intended work before editing.",
    ),
    (
        "obligations",
        "{} -> your registered scopes with required and reviewed revisions.",
    ),
    (
        "review",
        "{scope, revision, decision: Keep|Revise|Drop, note} -> after rereading a revision that was captured for you: by your capture call, or the capture in a host notice. The only way to clear an obligation; edit and offer take the same reviews inline.",
    ),
    (
        "post",
        "{task, scope: [scope id], work: \"None\" | {Revision: n} | {Checkpoint: id}, text, reply_to?: null | {event: seq, work}} -> advisory plan message to your peer. Only messages can be replied to: plans, live edits and checkpoint offers (Message events), never Reviewed, Obligation or other events. reply_to.work is the original message's work.",
    ),
    (
        "offer",
        "{id, task, revision, scope: [scope id], text, supersedes?: offer id, reviews?: [{scope, revision, decision, note}]} -> offer an exact captured revision for publication. Refused while any obligation is pending. reviews are checked and recorded first, as in edit.",
    ),
    ("withdraw", "{offer} -> withdraw your offer."),
    (
        "checkpoint",
        "{id: \"agent/offer-id\"} -> an offer's state (Available, Withdrawn, Superseded, Accepted).",
    ),
    (
        "candidate",
        "{revision} -> the exact candidate binding and members still missing an offer.",
    ),
    (
        "feedback",
        "{id, revision} -> queue required checks on a captured revision early; never publishes.",
    ),
    ("run", "{id} -> a check run with its results and outcome."),
    (
        "handle",
        "{through?: seq, deferred?: {seq: note}, processed?: [seq]} -> one atomic call. through handles every event in your stream after your cursor, up to that seq, as processed unless deferred lists it, and moves your cursor there; at or below the cursor it does nothing. It may be your own post's seq. The engine does not track what you were shown, so through also covers events no notice gave you. deferred keeps a note for an event at or below the new cursor; processed clears earlier deferrals. Never clears obligations.",
    ),
    (
        "recommend",
        "{} -> ask the engine whether to join, split or keep the current arrangement.",
    ),
    (
        "propose",
        "{action: Join|Split|Keep, text, failing: bool} -> propose a regrouping with your reasons.",
    ),
    (
        "respond",
        "{id, context, response: \"Agree\" | {Decline: {reason}}} -> answer a regrouping proposal; repeat its exact context.",
    ),
    (
        "job",
        "{id, revision, paths: [path]} -> run the host-enrolled gofmt over captured paths in isolation.",
    ),
    (
        "apply_job",
        "{id} -> apply a finished job's output through the normal freshness gate.",
    ),
];
/// MCP definitions for every agent-facing operation; each takes its request as its input.
pub fn tools() -> Value {
    json!(
        OPERATIONS
            .iter()
            .map(|(operation, help)| tool(operation, format!("Falinks {operation}: {help}")))
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
/// `{id, expected: {path: version}, output: {path: {content, executable?} | null}, reviews?}`.
fn edit(request: &Value) -> Result<Request> {
    let expected: BTreeMap<String, u64> = serde_json::from_value(request["expected"].clone())?;
    let mut output = BTreeMap::new();
    for (path, file) in request["output"]
        .as_object()
        .ok_or("request.output must map paths to {content} or null")?
    {
        let file = match file {
            Value::Null => Output::Delete,
            file => {
                require_keys(
                    file,
                    &format!("output {path:?}"),
                    &["content"],
                    &["executable"],
                )?;
                Output::Whole(File {
                    bytes: file["content"]
                        .as_str()
                        .ok_or("output content must be text")?
                        .as_bytes()
                        .to_vec(),
                    executable: file["executable"].as_bool().unwrap_or(false),
                })
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
        // shortcut: the edit tools take no note yet, so edits keep the old text; drop when #192 adds one.
        note: format!("request {id}"),
        reviews: reviews.unwrap_or_default(),
    })
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
