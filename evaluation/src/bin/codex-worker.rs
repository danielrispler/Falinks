//! Baseline-only Codex exec bridge. The host copies this binary into each fresh sandbox.
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::{fs, thread};

fn emit(value: Value) {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{value}")
        .and_then(|_| stdout.flush())
        .expect("host channel closed");
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut first = String::new();
    io::stdin().lock().read_line(&mut first)?;
    let start: Value = serde_json::from_str(&first)?;
    let (tx, events) = channel::<Value>();
    thread::spawn(move || {
        for line in io::stdin().lines().map_while(Result::ok) {
            let event: Value = serde_json::from_str(&line).expect("host sent invalid JSON");
            if event["type"] == "ok" && event["action"] == "usage" {
                continue; // Telemetry receipts must not wake a waiting model.
            }
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    let scratch = Path::new("scratch");
    let schema = scratch.join("response-schema.json");
    fs::write(&schema, json!({
        "type": "object", "additionalProperties": false,
        "properties": {
            "action": {"type": "string", "enum": ["message", "share", "ack", "check", "ready", "wait"]},
            "text": {"type": "string"}, "milestone": {"type": "string"},
            "commit": {"type": "string"}, "resolved": {"type": "string"}},
        "required": ["action", "text", "milestone", "commit", "resolved"],
    }).to_string())?;
    let mut thread_id: Option<String> = None;
    let mut pending = vec![start.clone()];
    let mut ready = false;
    let mut turn = 0;
    loop {
        if pending.is_empty() {
            pending.push(events.recv()?);
        }
        pending.extend(events.try_iter());
        if ready {
            loop {
                thread::park(); // Host terminates the process after final validation.
            }
        }
        turn += 1;
        let answer = scratch.join("answer.json");
        let prompt = format!(
            "You are one agent in the Git baseline. Implement work with your peer. \
             Use tools to read/edit your worktree and run focused checks. \
             Do not write your peer worktree. You may resolve merges in integration_workspace. \
             Return exactly one host action as JSON; empty strings for unused fields. \
             Host events are queued at turn boundaries, not mid-reasoning. \
             Do not perform Git commits yourself; share asks the host to capture your draft. \
             Do not edit while share capture is pending. Use wait if awaiting peer input. \
             No additional subagents, apps, browser, network tools or permission escalation.\n{}",
            Value::from(std::mem::take(&mut pending))
        );
        let model = &start["model"];
        let mut common: Vec<String> = [
            "--ignore-user-config",
            "--ignore-rules",
            "--json",
            "--output-schema",
        ]
        .map(String::from)
        .to_vec();
        common.extend([
            schema.to_string_lossy().into(),
            "-o".into(),
            answer.to_string_lossy().into(),
            "-m".into(),
            model["model"].as_str().unwrap_or_default().into(),
            "-c".into(),
            format!("model_reasoning_effort={}", model["reasoning"]),
            "-c".into(),
            "approval_policy=\"never\"".into(),
        ]);
        let mut argv = vec!["exec".to_string()];
        match &thread_id {
            Some(id) => argv.extend(
                ["resume".into()]
                    .into_iter()
                    .chain(common)
                    .chain([id.clone(), "-".into()]),
            ),
            None => argv.extend(
                ["--sandbox".into(), "workspace-write".into()]
                    .into_iter()
                    .chain(common)
                    .chain(["-".into()]),
            ),
        }
        let mut child = Command::new(
            start["runtime_binary"]
                .as_str()
                .ok_or("runtime_binary missing")?,
        )
        .args(&argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
        child.stdin.take().unwrap().write_all(prompt.as_bytes())?;
        let output = child.wait_with_output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Preserve runtime events in agent scratch; host retains stdout requests and stderr separately.
        fs::write(
            scratch.join(format!("turn-{turn}.jsonl")),
            stdout.as_bytes(),
        )?;
        for event in stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            if event["type"] == "thread.started" {
                thread_id = event["thread_id"].as_str().map(String::from);
            }
            if event["type"] == "turn.completed"
                && let Some(usage) = event["usage"].as_object()
            {
                let mut report = usage.clone();
                report.insert("turn".into(), turn.into());
                emit(json!({"action": "usage", "usage": report}));
            }
        }
        if !output.status.success() {
            // Generic runtime failures are infrastructure failures; no invented usage-limit diagnosis.
            return Err(format!("Codex exited {}", output.status).into());
        }
        let response: Value = serde_json::from_str(&fs::read_to_string(&answer)?)?;
        let action = response["action"]
            .as_str()
            .ok_or("response lacks action")?
            .to_string();
        if action == "wait" {
            continue;
        }
        let mut request = json!({"action": action});
        for field in ["text", "milestone", "commit", "resolved"] {
            if response[field].as_str().is_some_and(|v| !v.is_empty()) {
                request[field] = response[field].clone();
            }
        }
        emit(request);
        ready = action == "ready";
        let expected = match action.as_str() {
            "share" => "shared",
            "check" => "check_result",
            _ => "ok",
        };
        loop {
            let reply = events.recv()?;
            let done = reply["type"] == expected
                && (expected != "ok" || reply["action"] == action.as_str());
            pending.push(reply);
            if done {
                break;
            }
        }
    }
}
