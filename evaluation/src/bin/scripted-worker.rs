//! Test-only scripted workers and fake Codex runtime. They test orchestration, not model
//! performance; reference bytes are embedded by the trusted test host, never given to real workers.
//!
//! `scripted-worker relationships|conflict` speaks the host JSON-line protocol;
//! `scripted-worker exec ...` impersonates `codex exec` for the Codex bridge;
//! `scripted-worker -p ...` impersonates `claude -p` stream-json for the Claude bridge;
//! `scripted-worker <fixture> --output DIR ...` impersonates the Falinks-arm runner for batches.
use serde_json::{Value, json};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs, thread};

fn reference(fixture: &str, file: &str) -> String {
    let protected = match fixture {
        "go-page" => include_str!("../../protected/go-page.json"),
        _ => include_str!("../../protected/go-relationships.json"),
    };
    let value: Value = serde_json::from_str(protected).unwrap();
    value["reference"][file].as_str().unwrap().to_string()
}

fn send(value: Value) {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{value}").unwrap();
    stdout.flush().unwrap();
}

fn append(file: &str, text: &str) {
    let current = fs::read_to_string(file).unwrap();
    fs::write(file, current + text).unwrap();
}

fn park() -> ! {
    loop {
        thread::park();
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("relationships") => relationships(),
        Some("conflict") => conflict(),
        Some("exec") => fake_codex(&args),
        Some("-p") => fake_claude(&args),
        Some("rust-errors" | "go-page" | "go-relationships") => fake_arm(&args),
        other => panic!("unknown mode {other:?}"),
    }
}

fn lines() -> impl Iterator<Item = Value> {
    io::stdin()
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
}

fn relationships() {
    let mut events = lines();
    let start = events.next().unwrap();
    let agent = start["agent"].as_str().unwrap().to_string();
    let file = if agent == "A" {
        "producer.go"
    } else {
        "consumer.go"
    };
    // Resolve controller relative to the public worktree; protected by sandbox-exec.
    let controller = Path::new(start["workspace"].as_str().unwrap())
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("controller/events.jsonl");
    match fs::read(controller) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            send(json!({"action": "message", "text": "protected-read-denied"}))
        }
        other => panic!("controller readable or unexpected: {other:?}"),
    }
    append(file, &format!("\n// first unfinished draft {agent}\n"));
    send(json!({"action": "share"}));
    let (mut pending, mut last_final) = (Value::Null, false);
    for event in events {
        match (event["type"].as_str().unwrap(), event["action"].as_str()) {
            ("development", _) => {
                send(json!({"action": "ack", "milestone": event["id"]}));
                pending = event;
            }
            ("ok", Some("ack")) => {
                if pending["id"] == "record-contract" {
                    fs::write(
                        file,
                        reference("go-relationships", file) + "\n// unfinished contract draft\n",
                    )
                    .unwrap();
                } else {
                    fs::write(file, reference("go-relationships", file)).unwrap();
                    last_final = true;
                }
                send(json!({"action": "share"}));
            }
            ("shared", _) if last_final => {
                send(json!({"action": "ready", "commit": event["commit"]}))
            }
            ("ok", Some("ready")) => park(),
            _ => {}
        }
    }
}

fn conflict() {
    let mut events = lines();
    let start = events.next().unwrap();
    let agent = start["agent"].as_str().unwrap().to_string();
    append("handler.go", &format!("\n// initial {agent}\n"));
    send(json!({"action": "share"}));
    let (mut is_final, mut own_final, mut peer_final, mut checking) =
        (false, Value::Null, false, false);
    for event in events {
        match (event["type"].as_str().unwrap(), event["action"].as_str()) {
            ("development", _) => send(json!({"action": "ack", "milestone": event["id"]})),
            ("ok", Some("ack")) => {
                fs::write(
                    "handler.go",
                    reference("go-page", "handler.go") + &format!("\n// final {agent}\n"),
                )
                .unwrap();
                is_final = true;
                send(json!({"action": "share"}));
            }
            ("shared", _) if is_final => {
                own_final = event["commit"].clone();
                if agent == "B" {
                    send(json!({"action": "ready", "commit": own_final}));
                }
            }
            ("peer_diff", _)
                if event["sender"] == "B"
                    && event["diff"].as_str().unwrap().contains("+// final B") =>
            {
                peer_final = true
            }
            ("check_result", _) => {
                if event["conflict"].as_str().is_some_and(|c| !c.is_empty()) {
                    let integration =
                        PathBuf::from(start["integration_workspace"].as_str().unwrap());
                    fs::write(
                        integration.join("handler.go"),
                        reference("go-page", "handler.go") + "\n// resolved ordinary merge\n",
                    )
                    .unwrap();
                    for args in [
                        &["add", "handler.go"][..],
                        &["commit", "-m", "Resolve both contributions"],
                    ] {
                        let status = Command::new("git")
                            .args([
                                "-c",
                                "core.hooksPath=/dev/null",
                                "-c",
                                "user.name=Test",
                                "-c",
                                "user.email=test@example.invalid",
                            ])
                            .args(args)
                            .current_dir(&integration)
                            .stdout(Stdio::null())
                            .status()
                            .unwrap();
                        assert!(status.success());
                    }
                    let head = Command::new("git")
                        .args(["rev-parse", "HEAD"])
                        .current_dir(&integration)
                        .output()
                        .unwrap();
                    send(
                        json!({"action": "check", "resolved": String::from_utf8(head.stdout).unwrap().trim()}),
                    );
                } else if event["passed"] == true {
                    send(json!({"action": "ready", "commit": own_final}));
                } else {
                    panic!("resolved candidate failed");
                }
            }
            ("ok", Some("ready")) => park(),
            _ => {}
        }
        if agent == "A" && !own_final.is_null() && peer_final && !checking {
            checking = true;
            send(json!({"action": "check"}));
        }
    }
}

fn fake_codex(argv: &[String]) {
    let flag = |name: &str| argv[argv.iter().position(|a| a == name).unwrap() + 1].clone();
    assert_eq!(flag("-m"), "scripted");
    if argv.contains(&"resume".to_string()) {
        assert_eq!(argv[argv.len() - 2], "fixture-thread");
    }
    let mut prompt = String::new();
    io::stdin().read_to_string(&mut prompt).unwrap();
    let events: Vec<Value> = serde_json::from_str(prompt.rsplit_once('\n').unwrap().1).unwrap();
    // A telemetry acknowledgment alone is not new peer/task context.
    assert!(
        events
            .iter()
            .any(|e| e["type"] != "ok" || e["action"] != "usage"),
        "telemetry wait spin"
    );
    let has = |kind: &str, action: Option<&str>| {
        events
            .iter()
            .find(|e| e["type"] == kind && action.is_none_or(|a| e["action"] == a))
    };
    let state_file = Path::new("scratch/fake-state.json");
    let mut state: Value = fs::read_to_string(state_file)
        .map(|s| serde_json::from_str(&s).unwrap())
        .unwrap_or(json!({"phase": "start"}));
    let mut response =
        json!({"action": "wait", "text": "", "milestone": "", "commit": "", "resolved": ""});
    let phase = state["phase"].as_str().unwrap().to_string();
    if let Some(start) = has("start", None) {
        let file = if start["agent"] == "A" {
            "producer.go"
        } else {
            "consumer.go"
        };
        state["agent"] = start["agent"].clone();
        state["file"] = file.into();
        response["action"] = "message".into();
        response["text"] = format!("Plan: implement {file}").into();
        state["phase"] = "plan".into();
    } else if phase == "plan" && has("ok", Some("message")).is_some() {
        append(state["file"].as_str().unwrap(), "\n// unfinished draft\n");
        response["action"] = "share".into();
        state["phase"] = "initial".into();
    } else if let Some(development) = has("development", None) {
        response["action"] = "ack".into();
        response["milestone"] = development["id"].clone();
        state["phase"] = format!("ack-{}", development["id"].as_str().unwrap()).into();
    } else if phase.starts_with("ack-") && has("ok", Some("ack")).is_some() {
        let is_final = phase == "ack-follow-ups";
        let file = state["file"].as_str().unwrap().to_string();
        fs::write(
            &file,
            reference("go-relationships", &file)
                + if is_final {
                    ""
                } else {
                    "\n// contract draft\n"
                },
        )
        .unwrap();
        response["action"] = "share".into();
        state["phase"] = if is_final { "final" } else { "contract" }.into();
    } else if phase == "final"
        && let Some(shared) = has("shared", None)
    {
        response["action"] = "ready".into();
        response["commit"] = shared["commit"].clone();
        state["phase"] = "ready".into();
    }
    fs::write(state_file, state.to_string()).unwrap();
    fs::write(flag("-o"), response.to_string()).unwrap();
    println!(
        "{}",
        json!({"type": "thread.started", "thread_id": "fixture-thread"})
    );
    println!(
        "{}",
        json!({"type": "turn.completed", "usage": {"input_tokens": 2, "cached_input_tokens": 0, "output_tokens": 3}})
    );
}

/// Runs a host command the way the agent's Bash tool would.
fn host_command(args: &[&str]) -> (bool, Value) {
    let output = Command::new(env::var("FALINKS_HOST").unwrap())
        .arg("host")
        .args(args)
        .output()
        .unwrap();
    let reply = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (output.status.success(), reply)
}

fn fake_claude(argv: &[String]) {
    let flag = |name: &str| argv[argv.iter().position(|a| a == name).unwrap() + 1].clone();
    assert_eq!(flag("--model"), "scripted");
    assert_eq!(flag("--max-turns"), "12");
    let settings: Value = serde_json::from_str(&flag("--settings")).unwrap();
    assert_eq!(settings["sandbox"]["enabled"], false);
    let tools: Vec<String> = flag("--tools").split(',').map(String::from).collect();
    let agent = env::var("FALINKS_AGENT").unwrap();
    // The runtime runs in scratch/; the agent's worktree is its parent.
    let file = format!(
        "../{}",
        if agent == "A" {
            "producer.go"
        } else {
            "consumer.go"
        }
    );
    let mut first = true;
    for line in io::stdin().lines() {
        let message: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let text = message["message"]["content"].as_str().unwrap().to_string();
        send(
            json!({"type": "system", "subtype": "init", "model": "scripted",
            "claude_code_version": "scripted-v1", "permissionMode": "dontAsk", "tools": tools,
            "session_id": flag("--session-id")}),
        );
        let mut subtype = "success";
        if first {
            first = false;
            let (ok, reply) = host_command(&["ready"]);
            assert!(
                !ok && reply["type"] == "refused",
                "early ready accepted: {reply}"
            );
            // A host-side protocol mistake is refused, not an infrastructure failure.
            let (ok, reply) = host_command(&["ack", "no-such-development"]);
            assert!(
                !ok && reply["type"] == "refused",
                "bad ack accepted: {reply}"
            );
            append(&file, &format!("\n// first unfinished draft {agent}\n"));
            assert!(host_command(&["message", "Plan:", "draft", "first"]).0);
            assert!(host_command(&["share"]).0);
            // The bridge must continue a turn that hit the tool-call cap.
            subtype = "error_max_turns";
        } else if let Some(event) = text.strip_prefix("Host event: ") {
            let event: Value = serde_json::from_str(event).unwrap();
            if event["type"] == "development" {
                let id = event["id"].as_str().unwrap();
                assert!(host_command(&["ack", id]).0);
                let name = &file[3..];
                let draft = if id == "record-contract" {
                    "\n// unfinished contract draft\n"
                } else {
                    ""
                };
                fs::write(&file, reference("go-relationships", name) + draft).unwrap();
                assert!(host_command(&["share"]).0);
                if id == "follow-ups" {
                    let (ok, reply) = host_command(&["ready"]);
                    assert!(ok, "{reply}");
                }
            }
        }
        send(
            json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed",
            "unifiedWindows": {"five_hour": {"utilization": 0.1}}}}),
        );
        send(json!({"type": "assistant", "message": {"model": "scripted",
            "content": [{"type": "thinking", "thinking": "hidden"}, {"type": "text", "text": "done"}]}}));
        send(json!({"type": "result", "subtype": subtype, "num_turns": 1,
            "usage": {"input_tokens": 2, "output_tokens": 3}, "modelUsage": {"scripted": {}}}));
    }
}

/// Writes Falinks-arm evidence with `FALINKS_FAKE_OUTCOME` (default success), after checking
/// that the batch denied every earlier run to this one.
fn fake_arm(argv: &[String]) {
    let flag = |name: &str| argv[argv.iter().position(|a| a == name).unwrap() + 1].clone();
    let config: Value =
        serde_json::from_str(&fs::read_to_string(flag("--config")).unwrap()).unwrap();
    let output = PathBuf::from(flag("--output"));
    let denied = config["denied_roots"].as_array().unwrap();
    for sibling in fs::read_dir(output.parent().unwrap()).unwrap().flatten() {
        if sibling.path() != output {
            let path = sibling.path().to_string_lossy().into_owned();
            assert!(
                denied.contains(&Value::from(path.clone())),
                "{path} not denied"
            );
        }
    }
    fs::create_dir_all(output.join("controller")).unwrap();
    let outcome = env::var("FALINKS_FAKE_OUTCOME").unwrap_or("success".into());
    let result = json!({"run_id": output.file_name().unwrap().to_string_lossy(), "arm": "falinks",
        "fixture": argv[0], "pair": flag("--pair"), "scored": flag("--scored") == "true", "outcome": outcome,
        "timing": {"elapsed_seconds": 10.0}, "transitions": 0});
    fs::write(output.join("controller/result.json"), result.to_string()).unwrap();
}
