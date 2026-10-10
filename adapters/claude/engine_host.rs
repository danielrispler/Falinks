//! The production `falinks::Engine` behind authenticated tool envelopes (#26).
//! The boundary has already bound host identity; this maps each operation to the
//! engine as that agent's opaque `Client`. Agent mistakes are refusals; an identity
//! mismatch or an engine incident is an error that latches the worker's gate.
use falinks::{Client, Disposition, Engine, File, JobSpec, Outcome, Request};
use falinks_host::{Result, require};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

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
        let identity = &envelope["identity"];
        let root = self.engine.root(&self.client)?;
        require(
            identity["agent"] == self.agent.as_str()
                && identity["workspace"].as_str() == root.to_str(),
            "envelope identity is not this agent's current workspace",
        )?;
        let request = &envelope["request"];
        let outcome = self.dispatch(envelope["operation"].as_str().unwrap_or(""), request);
        // An unknown change halts the engine: stop this worker's capability too.
        if let Some(incident) = self.engine.incident()? {
            return Err(format!("engine incident: {}", incident.reason).into());
        }
        Ok(match outcome {
            Ok(value) => text(value),
            Err(error) => json!({"accepted":false,"reason":error.to_string()}),
        })
    }
    fn dispatch(&self, operation: &str, request: &Value) -> Result<Value> {
        let (engine, client) = (&self.engine, &self.client);
        Ok(match operation {
            "capture" => {
                let capture = engine.capture_for(client)?;
                let wanted: Vec<&str> = request["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let files: serde_json::Map<String, Value> = capture
                    .files
                    .iter()
                    .map(|(path, versioned)| {
                        let mut entry = json!({"version":versioned.version,
                            "exists":versioned.file.is_some()});
                        if let Some(file) = &versioned.file
                            && wanted.contains(&path.as_str())
                        {
                            entry["content"] = json!(String::from_utf8_lossy(&file.bytes));
                            entry["executable"] = json!(file.executable);
                        }
                        (path.clone(), entry)
                    })
                    .collect();
                json!({"accepted":true,"revision":capture.revision,"tree":capture.tree,
                    "space":capture.space,"files":files})
            }
            "edit" => {
                let outcome = engine.apply(client, edit(request)?)?;
                json!({"accepted":matches!(outcome, Outcome::Applied{..}),"outcome":outcome})
            }
            "retry" | "incorporate" => {
                let request = edit(request)?;
                let outcome = if operation == "retry" {
                    engine.retry(client, request)?
                } else {
                    engine.incorporate(client, request)?
                };
                json!({"accepted":matches!(outcome, Outcome::Applied{..}),"outcome":outcome})
            }
            "request" => {
                let record = engine.request(client, field(request, "id")?)?;
                json!({"accepted":record.is_some(),"record":record})
            }
            "register" => {
                json!({"accepted":true,"obligation":engine.register(client, parse(request)?)?})
            }
            "obligations" => json!({"accepted":true,"obligations":engine.obligations(client)?}),
            "review" => {
                json!({"accepted":true,"obligation":engine.review(client, parse(request)?)?})
            }
            "offer" => json!({"accepted":true,"checkpoint":engine.offer(client, parse(request)?)?}),
            "events" => {
                let after = request["after"].as_u64().unwrap_or(0);
                json!({"accepted":true,"events":engine.events(client, after)?})
            }
            "handle" => {
                let seq = number(request, "seq")?;
                let disposition = match request["deferred"].as_str() {
                    Some(note) => Disposition::Deferred { note: note.into() },
                    None => Disposition::Processed,
                };
                engine.handle(client, seq, disposition)?;
                json!({"accepted":true})
            }
            "pending" => {
                let pending = engine.pending(client)?;
                let deferred: Vec<Value> = pending
                    .deferred
                    .iter()
                    .map(|(event, note)| json!({"event":event,"note":note}))
                    .collect();
                json!({"accepted":true,"handled":pending.handled,"unhandled":pending.unhandled,
                    "deferred":deferred,"obligations":pending.obligations})
            }
            "post" => json!({"accepted":true,"event":engine.post(client, parse(request)?)?}),
            "withdraw" => {
                json!({"accepted":true,"checkpoint":engine.withdraw(client, field(request, "offer")?)?})
            }
            "checkpoint" => {
                let checkpoint = engine.checkpoint(field(request, "id")?)?;
                json!({"accepted":checkpoint.is_some(),"checkpoint":checkpoint})
            }
            "candidate" => {
                let candidate = engine.candidate(number(request, "revision")?)?;
                json!({"accepted":true,"binding":candidate.binding,"missing":candidate.missing})
            }
            "feedback" => {
                let run =
                    engine.feedback(client, field(request, "id")?, number(request, "revision")?)?;
                json!({"accepted":true,"run":run})
            }
            "run" => {
                let run = engine.run(field(request, "id")?)?;
                json!({"accepted":run.is_some(),"run":run})
            }
            "recommend" => json!({"accepted":true,"proposal":engine.recommend(client)?}),
            "propose" => {
                json!({"accepted":true,"proposal":engine.propose(client, parse(request)?)?})
            }
            "respond" => {
                let response = parse(&request["response"])?;
                let proposal = engine.respond(
                    client,
                    field(request, "id")?,
                    field(request, "context")?,
                    response,
                )?;
                json!({"accepted":true,"proposal":proposal})
            }
            "job" => {
                let keys = request.as_object().map(|r| r.keys().count());
                require(keys == Some(3), "job takes exactly id, revision and paths")?;
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
                let outcome = engine.apply_job(client, field(request, "id")?)?;
                json!({"accepted":matches!(outcome, Outcome::Applied{..}),"outcome":outcome})
            }
            _ => return Err(format!("unsupported operation {operation}").into()),
        })
    }
}

/// Agent-facing operations and their request shapes. Host-only calls (checks, validation,
/// workspaces, reconsideration, disconnect, run retry, space-0 capture) are never tools.
pub const OPERATIONS: &[(&str, &str)] = &[
    (
        "capture",
        "{paths?: [path]} -> your workspace's last completed revision, every file's version, and the text of the named paths. Native Read can show a partial installation; capture never does.",
    ),
    (
        "edit",
        "{id, expected: {path: version}, output: {path: {content, executable?} | null}} -> applies all outputs or nothing. expected names every input and output. Stale returns current versions; reread and use a new id. Unreviewed means review first.",
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
        "{scope, revision, decision: Keep|Revise|Drop, note} -> after rereading a captured revision. The only way to clear an obligation.",
    ),
    (
        "post",
        "{task, scope: [scope id], work: \"None\" | {Revision: n} | {Checkpoint: id}, text, reply_to: null | {event: seq, work}} -> advisory plan message to your peer.",
    ),
    (
        "offer",
        "{id, task, revision, scope: [scope id], text, supersedes?: offer id} -> offer an exact captured revision for publication. Refused while any obligation is pending.",
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
    ("events", "{after: seq} -> ordered events after a position."),
    (
        "handle",
        "{seq, deferred?: note} -> mark the next unhandled event processed, or deferred with a note. Never clears obligations.",
    ),
    (
        "pending",
        "{} -> unhandled events, deferred events and pending obligations.",
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
/// MCP definitions for every agent-facing operation, bound to the worker's workspace.
pub fn tools(workspace: &std::path::Path) -> Value {
    json!(OPERATIONS.iter().map(|(operation, help)| json!({
        "name":format!("falinks_{operation}"),
        "description":format!("Falinks {operation}: {help} Call with {{\"workspace\": \"{}\", \"request\": {{...}}}}.", workspace.display()),
        "inputSchema":{"type":"object","properties":{"workspace":{"type":"string"},"request":{"type":"object"}},
            "required":["workspace","request"],"additionalProperties":false}})).collect::<Vec<_>>())
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
/// `{id, expected: {path: version}, output: {path: {content, executable?} | null}}`.
fn edit(request: &Value) -> Result<Request> {
    let expected: BTreeMap<String, u64> = serde_json::from_value(request["expected"].clone())?;
    let mut output = BTreeMap::new();
    for (path, file) in request["output"]
        .as_object()
        .ok_or("request.output must map paths to {content} or null")?
    {
        let file = match file {
            Value::Null => None,
            file => Some(File {
                bytes: file["content"]
                    .as_str()
                    .ok_or("output content must be text")?
                    .as_bytes()
                    .to_vec(),
                executable: file["executable"].as_bool().unwrap_or(false),
            }),
        };
        output.insert(path.clone(), file);
    }
    Ok(Request {
        id: field(request, "id")?.into(),
        expected,
        output,
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
