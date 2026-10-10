//! Throwaway issue #32 probe: a stdio MCP server, a hook logger and a
//! stream-json driver for the pinned `claude` CLI. Research evidence only.
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::CommandExt,
    process::{self, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn append(log: &str, value: &Value) -> Result<()> {
    let mut file = fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(file, "{value}")?;
    Ok(())
}

fn claude_env() -> Value {
    env::vars()
        .filter(|(k, _)| {
            k.starts_with("CLAUDE") || k.starts_with("MCP") || k.starts_with("FALINKS")
        })
        .map(|(k, v)| (k, Value::String(v)))
        .collect()
}

fn arg(args: &[String], name: &str) -> Result<String> {
    let i = args
        .iter()
        .position(|a| a == name)
        .ok_or(format!("missing {name}"))?;
    Ok(args
        .get(i + 1)
        .ok_or(format!("missing {name} value"))?
        .clone())
}

fn tool(name: &str, description: &str, props: Value) -> Value {
    let required: Vec<&String> = props
        .as_object()
        .map(|o| o.keys().collect())
        .unwrap_or_default();
    json!({"name": name, "description": description,
           "inputSchema": {"type": "object", "properties": props, "required": required}})
}

/// Newline-delimited JSON-RPC MCP server. Logs every message with its process context.
fn mcp(log: &str) -> Result<()> {
    append(
        log,
        &json!({"t": now_ms(), "start": {"pid": process::id(),
        "ppid": std::os::unix::process::parent_id(), "cwd": env::current_dir()?, "env": claude_env()}}),
    )?;
    let s = || json!({"type": "string"});
    let tools = json!([
        tool(
            "falinks_edit",
            "Mediated source edit: replaces the enrolled source file with `request`.",
            json!({"workspace": s(), "request": s()})
        ),
        tool(
            "falinks_review",
            "Explicit revision-bound review of a pending event.",
            json!({"event": s(), "action": s(), "reviewed_hash": s()})
        ),
        tool(
            "falinks_offer",
            "Offer the exact checkpoint for combination; not publication.",
            json!({"expected_hash": s()})
        ),
    ]);
    let mut out = std::io::stdout();
    for line in BufReader::new(std::io::stdin()).lines() {
        let msg: Value = serde_json::from_str(&line?)?;
        append(log, &json!({"t": now_ms(), "in": msg}))?;
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let result = match msg["method"].as_str().unwrap_or("") {
            "initialize" => json!({"protocolVersion": msg["params"]["protocolVersion"],
                "capabilities": {"tools": {}}, "serverInfo": {"name": "falinks", "version": "0.0.0"}}),
            "tools/list" => json!({"tools": tools}),
            "tools/call" => {
                let args = &msg["params"]["arguments"];
                let mut text = format!("logged {}", msg["params"]["name"]);
                // The server is outside the worker sandbox: this is the mediated write path.
                if msg["params"]["name"] == "falinks_edit"
                    && let (Ok(path), Some(body)) =
                        (env::var("FALINKS_SOURCE"), args["request"].as_str())
                {
                    fs::write(&path, format!("{body}\n"))?;
                    text = format!("engine_updated {path}");
                }
                json!({"content": [{"type": "text", "text": text}]})
            }
            _ => json!({}),
        };
        writeln!(
            out,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "result": result})
        )?;
        out.flush()?;
    }
    Ok(())
}

/// Hook command: log input; stamp runtime identity into falinks MCP tool input.
fn hook(log: &str) -> Result<()> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    let input: Value = serde_json::from_str(&raw)?;
    append(
        log,
        &json!({"t": now_ms(), "hook": input, "env": claude_env(),
        "ppid": std::os::unix::process::parent_id()}),
    )?;
    let name = input["tool_name"].as_str().unwrap_or("");
    if input["hook_event_name"] == "PreToolUse" && name.starts_with("mcp__falinks__") {
        // Overwrite any caller-supplied identity with the runtime-provided one.
        let mut stamped = input["tool_input"].clone();
        stamped["falinks_session"] = input["session_id"].clone();
        stamped["falinks_tool_use"] = input["tool_use_id"].clone();
        println!(
            "{}",
            json!({"hookSpecificOutput": {"hookEventName": "PreToolUse",
            "permissionDecision": "allow", "updatedInput": stamped}})
        );
    }
    Ok(())
}

fn matches(line: &Value, cond: &str) -> bool {
    let blocks = || {
        line["message"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    match cond.split_once(':') {
        None if cond == "result" => line["type"] == "result",
        None if cond == "tool_result" => {
            line["type"] == "user" && blocks().iter().any(|b| b["type"] == "tool_result")
        }
        Some(("tool", name)) => {
            line["type"] == "assistant"
                && blocks().iter().any(|b| {
                    b["type"] == "tool_use" && b["name"].as_str().is_some_and(|n| n.contains(name))
                })
        }
        Some(("text", needle)) => line.to_string().contains(needle),
        _ => false,
    }
}

fn send(stdin: &mut impl Write, log: &str, msg: Value) -> Result<()> {
    writeln!(stdin, "{msg}")?;
    stdin.flush()?;
    append(log, &json!({"t": now_ms(), "in": msg}))
}

/// Drive `claude -p` over stream-json with a JSON step script; log both directions.
fn drive(args: &[String]) -> Result<()> {
    let log = arg(args, "--log")?;
    let root = env::var("FALINKS_ROOT").unwrap_or_default();
    let script: Vec<Value> = serde_json::from_str(
        &fs::read_to_string(arg(args, "--script")?)?.replace("{ROOT}", &root),
    )?;
    let split = args
        .iter()
        .position(|a| a == "--")
        .ok_or("missing -- claude args")?;
    let mut command = Command::new(&args[split + 1]);
    command
        .args(&args[split + 2..])
        .current_dir(arg(args, "--cwd")?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(fs::File::create(format!("{log}.stderr"))?)
        .process_group(0);
    // Keep the outer session's identity out of the worker.
    for (k, _) in env::vars().filter(|(k, _)| k.starts_with("CLAUDE")) {
        command.env_remove(k);
    }
    let mut child = command.spawn()?;
    let group = child.id() as i32;
    let mut stdin = child.stdin.take().ok_or("stdin")?;
    let stdout = child.stdout.take().ok_or("stdout")?;
    let (tx, rx) = mpsc::channel::<Value>();
    let out_log = log.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            let value = serde_json::from_str(&line).unwrap_or(Value::String(line));
            let _ = append(&out_log, &json!({"t": now_ms(), "out": value}));
            if tx.send(value).is_err() {
                break;
            }
        }
    });
    let mut seen = Vec::<Value>::new();
    'steps: for step in script {
        append(&log, &json!({"t": now_ms(), "step": step}))?;
        if let Some(text) = step["send"].as_str() {
            send(
                &mut stdin,
                &log,
                json!({"type": "user", "message": {"role": "user", "content": text}}),
            )?;
        } else if step["interrupt"].as_bool() == Some(true) {
            send(
                &mut stdin,
                &log,
                json!({"type": "control_request",
                "request_id": format!("i{}", now_ms()), "request": {"subtype": "interrupt"}}),
            )?;
        } else if let Some(ms) = step["sleep_ms"].as_u64() {
            thread::sleep(Duration::from_millis(ms));
        } else if let Some(cond) = step["wait"].as_str() {
            // `count` waits for the Nth occurrence over the whole run.
            let count = step["count"].as_u64().unwrap_or(1) as usize;
            let deadline =
                Instant::now() + Duration::from_secs(step["timeout_s"].as_u64().unwrap_or(300));
            while seen.iter().filter(|l| matches(l, cond)).count() < count {
                match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(line) => seen.push(line),
                    Err(_) => {
                        append(&log, &json!({"t": now_ms(), "timeout": cond}))?;
                        break 'steps;
                    }
                }
            }
        }
    }
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait()?.is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(200));
    }
    // SAFETY: killpg takes no pointers; the group was created for this child.
    unsafe { libc::killpg(group, libc::SIGKILL) };
    let status = child.wait()?;
    append(&log, &json!({"t": now_ms(), "exit": status.code()}))?;
    append(
        &log,
        &json!({"t": now_ms(), "model_check": model_check(&seen, args)}),
    )?;
    Ok(())
}

/// Mandatory: every init and every result's model usage must name only the
/// requested model, with no refusal fallback. Any change invalidates the run.
fn model_check(seen: &[Value], args: &[String]) -> Value {
    let expected = arg(args, "--model").ok();
    let mut models = std::collections::BTreeSet::new();
    let mut fallback = false;
    for line in seen {
        if line["type"] == "system" && line["subtype"] == "init" {
            models.insert(line["model"].as_str().unwrap_or("?").to_string());
        }
        if line["type"] == "system" && line["subtype"] == "model_refusal_fallback" {
            fallback = true;
        }
        if let Some(usage) = line["modelUsage"].as_object() {
            models.extend(usage.keys().cloned());
        }
    }
    let passed =
        !fallback && models.len() == 1 && expected.as_ref().is_none_or(|m| models.contains(m));
    json!({"passed": passed, "expected": expected, "models": models, "refusal_fallback": fallback})
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("mcp") => arg(&args, "--log").and_then(|l| mcp(&l)),
        Some("hook") => arg(&args, "--log").and_then(|l| hook(&l)),
        Some("drive") => drive(&args),
        _ => Err("usage: claude-probe mcp|hook --log FILE | drive --script F --log F --cwd D -- claude ARGS".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        process::exit(1);
    }
}
