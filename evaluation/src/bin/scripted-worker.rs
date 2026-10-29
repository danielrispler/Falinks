//! Test-only scripted workers and fake Codex runtime. They test orchestration, not model
//! performance; reference bytes are embedded by the trusted test host, never given to real workers.
//!
//! `scripted-worker relationships [received|late|never]|conflict` speaks the host JSON-line
//! protocol; with a usage mode, `relationships` reports a turn at start and its final result
//! after `ready` at once, only when the host stops it, or never; alone, it writes both files;
//! `scripted-worker exec ...` impersonates `codex exec` for the Codex bridge;
//! `scripted-worker -p --output-format json ...` impersonates the judge's `claude -p` call: it
//! appends the prompt (after `-p`) to `$FALINKS_FAKE_JUDGE/prompts.json` and replies with the next canned
//! text in `$FALINKS_FAKE_JUDGE/replies.json`, where `null` is a failed call;
//! `scripted-worker -p ...` impersonates `claude -p` stream-json for the Claude bridge; copied
//! as `launches-PROGRAM`, its first turn runs `PROGRAM` through Bash (a nested agent);
//! `scripted-worker <fixture> --output DIR ...` impersonates the Falinks-arm runner for batches;
//! `FALINKS_FAKE_NESTED=COMMAND` records that command as a nested agent.
use serde_json::{Value, json};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{env, fs, thread};

fn references(fixture: &str) -> Value {
    let protected = match fixture {
        "go-page" => include_str!("../../protected/go-page.json"),
        _ => include_str!("../../protected/go-relationships.json"),
    };
    let value: Value = serde_json::from_str(protected).unwrap();
    value["reference"].clone()
}

fn reference(fixture: &str, file: &str) -> String {
    references(fixture)[file].as_str().unwrap().to_string()
}

/// Writes an agent's final go-relationships files beside `file`: A owns `producer.go`, B every
/// other reference file, including its follow-up's own file.
fn write_final_files(file: &str) {
    let path = Path::new(file);
    let producer = path.file_name().unwrap() == "producer.go";
    for (name, text) in references("go-relationships").as_object().unwrap() {
        if (name == "producer.go") == producer {
            fs::write(path.with_file_name(name), text.as_str().unwrap()).unwrap();
        }
    }
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
        Some("relationships") => relationships(args.get(1).map(String::as_str)),
        Some("conflict") => conflict(),
        Some("exec") => fake_codex(&args),
        Some("-p") if args.windows(2).any(|w| w == ["--output-format", "json"]) => {
            fake_judge(&args)
        }
        Some("-p") => fake_claude(&args),
        Some(
            "rust-errors" | "go-page" | "go-relationships" | "go-indep-2" | "go-indep-4"
            | "go-invoice",
        ) => fake_arm(&args),
        other => panic!("unknown mode {other:?}"),
    }
}

fn lines() -> impl Iterator<Item = Value> {
    io::stdin()
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
}

static STOPPING: AtomicBool = AtomicBool::new(false);

extern "C" fn stopping(_: libc::c_int) {
    STOPPING.store(true, Ordering::SeqCst);
}

/// A turn result like the Claude bridge's.
fn usage(turn: u64) {
    send(json!({"action": "usage", "usage": {"turn": turn, "usage": {"output_tokens": 3}}}));
}

fn relationships(usage_mode: Option<&str>) {
    if usage_mode.is_some() {
        usage(1);
    }
    let mut events = lines();
    let start = events.next().unwrap();
    let agent = start["agent"].as_str().unwrap().to_string();
    let files = match (start["peer_workspace"].is_null(), agent.as_str()) {
        (true, _) => vec!["producer.go", "consumer.go"],
        (_, "A") => vec!["producer.go"],
        _ => vec!["consumer.go"],
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
    for file in &files {
        append(file, &format!("\n// first unfinished draft {agent}\n"));
    }
    send(json!({"action": "share"}));
    let (mut pending, mut last_final) = (Value::Null, false);
    for event in events {
        match (event["type"].as_str().unwrap(), event["action"].as_str()) {
            ("development", _) => {
                send(json!({"action": "ack", "milestone": event["id"]}));
                pending = event;
            }
            ("ok", Some("ack")) => {
                let contract = pending["id"] == "record-contract";
                for file in &files {
                    if contract {
                        let draft = "\n// unfinished contract draft\n";
                        fs::write(file, reference("go-relationships", file) + draft).unwrap();
                    } else {
                        write_final_files(file);
                    }
                }
                last_final = !contract;
                send(json!({"action": "share"}));
            }
            ("shared", _) if last_final => {
                send(json!({"action": "ready", "commit": event["commit"]}))
            }
            ("ok", Some("ready")) => match usage_mode {
                Some("received") => {
                    usage(2);
                    park()
                }
                Some("late") => {
                    // SAFETY: the handler only stores an atomic flag.
                    unsafe {
                        libc::signal(libc::SIGTERM, stopping as *const () as libc::sighandler_t)
                    };
                    while !STOPPING.load(Ordering::SeqCst) {
                        thread::sleep(Duration::from_millis(10));
                    }
                    usage(2);
                    std::process::exit(0)
                }
                _ => park(),
            },
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
        if is_final {
            write_final_files(&file);
        } else {
            fs::write(
                &file,
                reference("go-relationships", &file) + "\n// contract draft\n",
            )
            .unwrap();
        }
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

/// Runs a host command the way the agent's Bash tool would: `"$FALINKS_HOST" ACTION ...`.
fn host_command(args: &[&str]) -> (bool, Value) {
    let output = Command::new(env::var("FALINKS_HOST").unwrap())
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
    // The real runtime finds its Keychain login by `USER`; without it the session is expired.
    assert!(
        env::var_os("USER").is_some(),
        "runtime launched without USER"
    );
    // Like the real runtime, Bash first creates its temp folder (default `/tmp/claude-<uid>`).
    let tmp = env::var("CLAUDE_CODE_TMPDIR").unwrap_or("/tmp".into());
    fs::create_dir_all(Path::new(&tmp).join("claude-scripted"))
        .expect("runtime temp folder not writable");
    // The real runtime reports its tools sorted, not in `--tools` order.
    let mut tools: Vec<String> = flag("--tools").split(',').map(String::from).collect();
    tools.sort();
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
        let program = env::args().next().unwrap();
        if let Some(launched) = program
            .rsplit('/')
            .next()
            .unwrap()
            .strip_prefix("launches-")
        {
            send(
                json!({"type": "assistant", "message": {"model": "scripted", "content": [
                {"type": "tool_use", "id": "t1", "name": "Bash",
                 "input": {"command": format!("{launched} -p 'help me'")}}]}}),
            );
        }
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
                if id == "record-contract" {
                    let draft = "\n// unfinished contract draft\n";
                    fs::write(&file, reference("go-relationships", name) + draft).unwrap();
                } else {
                    write_final_files(&file);
                }
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

fn fake_judge(argv: &[String]) {
    // The judge runs with no tools and the workers' isolation flags.
    for flags in [
        &["--tools", ""][..],
        &["--setting-sources", ""],
        &["--strict-mcp-config", "--disable-slash-commands"],
    ] {
        assert!(argv.windows(flags.len()).any(|w| w == flags), "{argv:?}");
    }
    let dir = PathBuf::from(env::var("FALINKS_FAKE_JUDGE").unwrap());
    let state = |name: &str| -> Vec<Value> {
        serde_json::from_str(&fs::read_to_string(dir.join(name)).unwrap_or("[]".into())).unwrap()
    };
    let mut prompts = state("prompts.json");
    prompts.push(argv[1].as_str().into());
    fs::write(dir.join("prompts.json"), Value::from(prompts).to_string()).unwrap();
    let mut replies = state("replies.json");
    let reply = replies.remove(0);
    fs::write(dir.join("replies.json"), Value::from(replies).to_string()).unwrap();
    let Some(text) = reply.as_str() else {
        eprintln!("scripted judge failure");
        std::process::exit(1);
    };
    send(
        json!({"type": "result", "subtype": "success", "is_error": false, "result": text,
        "usage": {"input_tokens": 5, "output_tokens": 7}, "modelUsage": {"claude-opus-5-5": {}}}),
    );
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
    let mut result = json!({"run_id": output.file_name().unwrap().to_string_lossy(), "arm": "falinks",
        "fixture": argv[0], "pair": flag("--pair"), "scored": flag("--scored") == "true", "outcome": outcome,
        "timing": {"elapsed_seconds": 10.0}, "transitions": 0});
    if let Ok(command) = env::var("FALINKS_FAKE_NESTED") {
        result["nested_agent"] = json!({"command": command});
        result["outcome"] = "run_invalid".into();
    }
    fs::write(output.join("controller/result.json"), result.to_string()).unwrap();
}
