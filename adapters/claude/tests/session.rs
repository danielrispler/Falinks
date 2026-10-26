use falinks_claude::{Launch, MODEL, Session};
use falinks_host::{EngineOutcome, Notice};
use serde_json::{Value, json};
use std::{cell::RefCell, path::Path, rc::Rc};

const SESSION: &str = "19a2d42a-a536-4dcb-9fb2-6de7a59d7699";
const TOKEN: &str = "host-token";

/// Shapes recorded from the pinned binary in #32 (`2026-10-10-probe.json`).
fn init(workspace: &Path) -> Value {
    json!({"type":"system","subtype":"init","claude_code_version":"2.1.308","cwd":workspace.with_file_name("scratch"),
        "model":MODEL,"permissionMode":"dontAsk","session_id":SESSION,"tools":Launch::fixture(workspace.parent().unwrap()).allowed(),
        "mcp_servers":[{"name":"falinks","source":"dynamic","status":"connected"}],
        "plugins":[{"name":"cc-plugin-agents-md","path":"builtin","source":"cc-plugin-agents-md@builtin"},
            {"name":"cc-plugin-telemetry","path":"builtin","source":"cc-plugin-telemetry@builtin"},
            {"name":"cc-plugin-plugin-authoring","path":"builtin","source":"cc-plugin-plugin-authoring@builtin"}],
        "skills":[],"slash_commands":[],"apiKeySource":"none"})
}
fn result() -> Value {
    json!({"type":"result","subtype":"success","session_id":SESSION,"permission_denials":[],
        "modelUsage":{MODEL:{"outputTokens":1}}})
}
fn hook(prompt: &str, tool_use: &str, tool: &str, input: &Value) -> Value {
    json!({"kind":"hook","token":TOKEN,"session":SESSION,"prompt_id":prompt,"tool_use_id":tool_use,
        "tool_name":format!("mcp__falinks__{tool}"),"tool_input":input})
}
fn call(tool_use: &str, tool: &str, input: &Value) -> Value {
    json!({"kind":"call","token":TOKEN,"session":SESSION,"tool_use_id":tool_use,"tool":tool,"arguments":input})
}

struct Fixture {
    _dir: tempfile::TempDir,
    workspace: std::path::PathBuf,
    calls: Rc<RefCell<Vec<Value>>>,
    session: Session,
}
fn fixture() -> Fixture {
    fixture_with(None)
}
fn fixture_with(notice: Option<&'static str>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().canonicalize().unwrap().join("source");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(dir.path().join("controller")).unwrap();
    std::fs::create_dir(dir.path().join("scratch")).unwrap();
    let calls = Rc::new(RefCell::new(vec![]));
    let seen = Rc::clone(&calls);
    let engine = Box::new(move |envelope: &Value| {
        seen.borrow_mut().push(envelope.clone());
        Ok(EngineOutcome {
            result: json!({"accepted":true}),
            notice: notice.map(|event| Notice {
                event: event.into(),
                context: json!({"revision":"R1"}),
            }),
            ..Default::default()
        })
    });
    let session = Session::new(
        &Launch::fixture(dir.path().canonicalize().unwrap().as_path()),
        "agent-a",
        SESSION,
        TOKEN,
        engine,
        false,
    )
    .unwrap();
    Fixture {
        _dir: dir,
        workspace,
        calls,
        session,
    }
}
fn edit() -> Value {
    json!({"request_id":"edit-1","content":"new"})
}

#[test]
fn stamped_call_in_a_verified_turn_reaches_engine_with_host_identity() {
    let mut f = fixture();
    let input = edit();
    f.session.line(&init(&f.workspace)).unwrap();
    assert_eq!(
        f.session
            .host(&hook("P1", "toolu_1", "falinks_edit", &input))
            .0,
        json!({"allow":true})
    );
    let (reply, notice) = f.session.host(&call("toolu_1", "falinks_edit", &input));
    assert_eq!(reply, json!({"accepted":true}));
    assert!(notice.is_none());
    assert_eq!(
        f.calls.borrow()[0]["identity"],
        json!({"agent":"agent-a","session":f.session.connection(),"thread":SESSION,"turn":"P1","workspace":f.workspace})
    );
    f.session.line(&result()).unwrap();
    assert!(!f.session.gate.failed);
}

#[test]
fn unstamped_forged_or_foreign_calls_never_reach_engine() {
    let mut f = fixture();
    let input = edit();
    let rejected = |reply: Value| reply["accepted"] == false || reply["allow"] == false;
    // Outside a verified turn, even a stamp is refused.
    assert!(rejected(
        f.session
            .host(&hook("P1", "toolu_0", "falinks_edit", &input))
            .0
    ));
    f.session.line(&init(&f.workspace)).unwrap();
    // No hook stamp.
    assert!(rejected(
        f.session.host(&call("toolu_1", "falinks_edit", &input)).0
    ));
    // Wrong host-channel token.
    let mut forged = hook("P1", "toolu_2", "falinks_edit", &input);
    forged["token"] = json!("guessed");
    assert!(rejected(f.session.host(&forged).0));
    // Another runtime session.
    let mut foreign = hook("P1", "toolu_3", "falinks_edit", &input);
    foreign["session"] = json!("other-session");
    assert!(rejected(f.session.host(&foreign).0));
    // Stamp for different input or tool than the call carries.
    f.session
        .host(&hook("P1", "toolu_4", "falinks_edit", &input));
    let mut changed = input.clone();
    changed["content"] = json!("swapped");
    assert!(rejected(
        f.session.host(&call("toolu_4", "falinks_edit", &changed)).0
    ));
    f.session
        .host(&hook("P1", "toolu_5", "falinks_edit", &input));
    assert!(rejected(
        f.session.host(&call("toolu_5", "falinks_offer", &input)).0
    ));
    // Caller-supplied identity, a workspace included.
    for (index, arguments) in [
        json!({"author":"agent-b"}),
        json!({"workspace":"/unauthorized","content":"x"}),
        json!({"content":"x","session":"forged"}),
    ]
    .iter()
    .enumerate()
    {
        let id = format!("toolu_6{index}");
        f.session.host(&hook("P1", &id, "falinks_edit", arguments));
        assert!(rejected(
            f.session.host(&call(&id, "falinks_edit", arguments)).0
        ));
    }
    // A stamp from an ended turn cannot open a new one.
    f.session
        .host(&hook("P1", "toolu_7", "falinks_edit", &input));
    f.session.host(&call("toolu_7", "falinks_edit", &input));
    f.session.line(&result()).unwrap();
    f.session.line(&init(&f.workspace)).unwrap();
    f.session
        .host(&hook("P1", "toolu_8", "falinks_edit", &input));
    assert!(rejected(
        f.session.host(&call("toolu_8", "falinks_edit", &input)).0
    ));
    assert_eq!(f.calls.borrow().len(), 1);
    // Rejections refuse the call without latching the runtime.
    assert!(!f.session.gate.failed);
}

#[test]
fn runtime_mismatch_latches_failure_and_blocks_engine() {
    let changed = |key: &'static str, value: Value| {
        move |w: &Path| {
            let mut line = init(w);
            line[key] = value.clone();
            vec![line]
        }
    };
    type Lines = Box<dyn Fn(&Path) -> Vec<Value>>;
    let cases: Vec<Lines> = vec![
        Box::new(changed("model", json!("claude-opus-4-8"))),
        Box::new(changed(
            "tools",
            json!([
                "Bash",
                "Glob",
                "Grep",
                "Read",
                "Write",
                "mcp__falinks__falinks_edit",
                "mcp__falinks__falinks_offer"
            ]),
        )),
        Box::new(changed(
            "mcp_servers",
            json!([{"name":"falinks","source":"dynamic","status":"connected"},{"name":"github","status":"connected"}]),
        )),
        Box::new(changed("skills", json!(["deploy"]))),
        Box::new(changed("claude_code_version", json!("2.1.288"))),
        Box::new(changed("session_id", json!("other-session"))),
        // Outside scratch: the live source, a link to it from scratch, and a sibling (#110).
        Box::new(|w| {
            let mut line = init(w);
            line["cwd"] = json!(w);
            vec![line]
        }),
        Box::new(|w| {
            let link = w.with_file_name("scratch").join("link");
            std::os::unix::fs::symlink(w, &link).unwrap();
            let mut line = init(w);
            line["cwd"] = json!(link);
            vec![line]
        }),
        Box::new(|w| {
            let sibling = w.with_file_name("scratch2");
            std::fs::create_dir(&sibling).unwrap();
            let mut line = init(w);
            line["cwd"] = json!(sibling);
            vec![line]
        }),
        Box::new(|w| {
            vec![
                init(w),
                json!({"type":"system","subtype":"model_refusal_fallback"}),
            ]
        }),
        Box::new(|w| {
            let mut r = result();
            r["modelUsage"] = json!({MODEL:{},"claude-opus-4-8":{}});
            vec![init(w), r]
        }),
        Box::new(|w| {
            vec![
                init(w),
                json!({"type":"assistant","message":{"model":MODEL,
                "content":[{"type":"tool_use","name":"WebFetch","input":{}}]}}),
            ]
        }),
        Box::new(|w| vec![init(w), json!({"type":"system","subtype":"unrecorded"})]),
        Box::new(|w| vec![init(w), init(w)]),
    ];
    for (index, case) in cases.iter().enumerate() {
        let mut f = fixture();
        let lines = case(&f.workspace);
        // Feed every line: a later line may be the one that fails.
        let outcomes: Vec<_> = lines.iter().map(|line| f.session.line(line)).collect();
        let failed = outcomes.iter().any(Result::is_err);
        assert!(failed && f.session.gate.failed, "case {index}");
        let input = edit();
        f.session
            .host(&hook("P1", "toolu_1", "falinks_edit", &input));
        assert_eq!(
            f.session.host(&call("toolu_1", "falinks_edit", &input)).0["accepted"],
            false,
            "case {index}"
        );
        assert!(f.calls.borrow().is_empty(), "case {index}");
    }
}

/// A nested agent started through Bash, or a session the host did not launch, makes the run
/// invalid and is recorded (#178 C7).
#[test]
fn a_nested_agent_latches_failure_and_is_recorded() {
    let bash = |command: &str| {
        json!({"type":"assistant","message":{"model":MODEL,
            "content":[{"type":"tool_use","name":"Bash","input":{"command":command}}]}})
    };
    for command in [
        "claude -p hi",
        "cd .. && node -e 1",
        "FOO=1 npx tsx x.ts",
        "/opt/pinned/claude-2.1.304 -p x",
    ] {
        let mut f = fixture();
        f.session.runtime = "/opt/pinned/claude-2.1.304".into();
        f.session.line(&init(&f.workspace)).unwrap();
        let error = f.session.line(&bash(command)).unwrap_err().to_string();
        assert!(error.contains("run invalid"), "{command}: {error}");
        assert!(f.session.gate.failed, "{command}");
        assert_eq!(f.session.nested_agents, [json!({"command": command})]);
    }
    for command in ["go test ./...", "cat node_modules.txt", "echo claude"] {
        let mut f = fixture();
        f.session.line(&init(&f.workspace)).unwrap();
        f.session.line(&bash(command)).unwrap();
        assert!(f.session.nested_agents.is_empty(), "{command}");
    }

    let mut f = fixture();
    let project = f.workspace.with_file_name("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join(format!("{SESSION}.jsonl")), "").unwrap();
    f.session.line(&init(&f.workspace)).unwrap();
    // init sets `project` from memory_paths, absent here.
    f.session.project = Some(project.clone());
    f.session.line(&result()).unwrap();
    std::fs::write(project.join("0000-foreign.jsonl"), "").unwrap();
    f.session.line(&init(&f.workspace)).unwrap();
    f.session.project = Some(project);
    assert!(f.session.line(&result()).is_err());
    assert_eq!(
        f.session.nested_agents,
        [json!({"session": "0000-foreign"})]
    );
}

/// Bash keeps its cwd across turns while it stays beneath scratch (#110).
#[test]
fn a_later_turn_may_start_beneath_scratch() {
    let mut f = fixture();
    let moved = f.workspace.with_file_name("scratch").join("w");
    std::fs::create_dir(&moved).unwrap();
    f.session.line(&init(&f.workspace)).unwrap();
    f.session.line(&result()).unwrap();
    let mut line = init(&f.workspace);
    line["cwd"] = json!(moved);
    f.session.line(&line).unwrap();
    let input = edit();
    assert_eq!(
        f.session
            .host(&hook("P2", "toolu_1", "falinks_edit", &input))
            .0,
        json!({"allow":true})
    );
    assert_eq!(
        f.session.host(&call("toolu_1", "falinks_edit", &input)).0,
        json!({"accepted":true})
    );
    assert!(!f.session.gate.failed);
}

fn replay(text: &str) -> Value {
    json!({"type":"user","isReplay":true,"message":{"role":"user","content":text}})
}

#[test]
fn delivery_is_presentation_only_and_records_its_turn() {
    let mut f = fixture_with(Some("E1"));
    let input = edit();
    f.session.sent("prompt", None);
    assert!(!f.session.idle());
    f.session.line(&init(&f.workspace)).unwrap();
    f.session.line(&replay("prompt")).unwrap();
    f.session
        .host(&hook("P1", "toolu_1", "falinks_edit", &input));
    let notice = f
        .session
        .host(&call("toolu_1", "falinks_edit", &input))
        .1
        .unwrap();
    f.session
        .boundary
        .enqueue(&notice.event, &notice.context)
        .unwrap();
    // Mid-turn send: presented at the next tool-result boundary of the same turn.
    f.session.sent("notice E1", Some("E1"));
    f.session.line(&replay("notice E1")).unwrap();
    f.session.line(&result()).unwrap();
    assert!(f.session.idle());
    // Completion race: a message sent after `result` lands in a new turn.
    f.session.sent("notice E1 again", Some("E1"));
    assert!(!f.session.idle());
    f.session.line(&init(&f.workspace)).unwrap();
    f.session.line(&replay("notice E1 again")).unwrap();
    f.session.line(&result()).unwrap();
    let spans: Vec<_> = f
        .session
        .deliveries
        .iter()
        .map(|d| {
            (
                d["sent_in_turn"].clone(),
                d["sent_span"].clone(),
                d["presented_span"].clone(),
            )
        })
        .collect();
    assert_eq!(
        spans,
        [
            (json!(true), json!(1), json!(1)),
            (json!(false), json!(1), json!(2))
        ]
    );
    // Neither send nor presentation clears the obligation.
    assert_eq!(
        f.session.boundary.pending().unwrap()[0]["status"],
        "pending"
    );
}

#[test]
fn only_the_pinned_claude_executable_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let impostor = dir.path().join("claude");
    std::fs::write(&impostor, b"#!/bin/sh\necho '2.1.308 (Claude Code)'\n").unwrap();
    let error = falinks_claude::verify_binary(&impostor)
        .unwrap_err()
        .to_string();
    // An unpinned platform refuses before hashing; a pinned one rejects the hash.
    let expected = match falinks_claude::pin(std::env::consts::OS, std::env::consts::ARCH) {
        Ok(_) => "SHA-256",
        Err(_) => "unsupported platform",
    };
    assert!(error.contains(expected), "{error}");
}

/// The launch flag and the per-turn `init` check name the same native tools.
#[test]
fn the_launch_flag_offers_exactly_the_native_tools_init_expects() {
    let args = falinks_claude::launch_args(Path::new("/c"), SESSION, false);
    let tools = &args[args.iter().position(|a| a == "--tools").unwrap() + 1];
    assert_eq!(tools, "Bash,Glob,Grep,Read");
    let launch = Launch::fixture(Path::new("/root"));
    let native: Vec<String> = launch
        .allowed()
        .into_iter()
        .filter(|t| !t.starts_with("mcp__"))
        .collect();
    assert_eq!(tools.split(',').collect::<Vec<_>>(), native);
}

#[test]
fn the_launch_tool_profile_sets_init_and_registered_operations() {
    let mut f = fixture();
    let mut launch = Launch::fixture(f.workspace.parent().unwrap());
    launch.tools = json!([{"name":"falinks_capture"},{"name":"falinks_edit"}]);
    assert_eq!(
        launch.allowed(),
        [
            "Bash",
            "Glob",
            "Grep",
            "Read",
            "mcp__falinks__falinks_capture",
            "mcp__falinks__falinks_edit"
        ]
    );
    let engine = Box::new(|_: &Value| {
        Ok(EngineOutcome {
            result: json!({"accepted":true}),
            notice: None,
            ..Default::default()
        })
    });
    f.session = Session::new(&launch, "agent-a", SESSION, TOKEN, engine, false).unwrap();
    // The historical three-tool init no longer matches this launch.
    assert!(f.session.line(&init(&f.workspace)).is_err());

    let mut f = fixture();
    let launch = Launch {
        tools: launch.tools.clone(),
        ..Launch::fixture(f.workspace.parent().unwrap())
    };
    f.session = Session::new(
        &launch,
        "agent-a",
        SESSION,
        TOKEN,
        Box::new(|_: &Value| {
            Ok(EngineOutcome {
                result: json!({"accepted":true}),
                notice: None,
                ..Default::default()
            })
        }),
        false,
    )
    .unwrap();
    let mut line = init(&f.workspace);
    line["tools"] = json!(launch.allowed());
    f.session.line(&line).unwrap();
    let input = json!({});
    f.session
        .host(&hook("P1", "toolu_1", "falinks_capture", &input));
    assert_eq!(
        f.session
            .host(&call("toolu_1", "falinks_capture", &input))
            .0,
        json!({"accepted":true})
    );
    f.session
        .host(&hook("P1", "toolu_3", "falinks_edit", &input));
    assert_eq!(
        f.session.host(&call("toolu_3", "falinks_edit", &input)).0,
        json!({"accepted":true})
    );
    // An operation outside the launch is refused even with a valid stamp.
    f.session
        .host(&hook("P1", "toolu_2", "falinks_offer", &input));
    assert_eq!(
        f.session.host(&call("toolu_2", "falinks_offer", &input)).0["accepted"],
        false
    );
}

#[test]
fn queued_messages_merged_into_one_presentation_each_count_as_presented() {
    let mut f = fixture();
    f.session.sent("prompt", None);
    f.session.line(&init(&f.workspace)).unwrap();
    f.session.line(&replay("prompt")).unwrap();
    // Two notices sent during one tool call reach the model as one replayed message.
    f.session.sent("notice E1", Some("E1"));
    f.session.sent("notice E2", Some("E2"));
    f.session.line(&replay("notice E1\nnotice E2")).unwrap();
    f.session.line(&result()).unwrap();
    assert!(f.session.idle(), "both merged notices were presented");
    let spans: Vec<_> = f
        .session
        .deliveries
        .iter()
        .map(|d| d["presented_span"].clone())
        .collect();
    assert_eq!(spans, [json!(1), json!(1)]);
}

#[test]
fn only_declared_platforms_have_a_claude_pin() {
    // Derived from PINS so promoting a platform never edits this test. Which platforms
    // are pinned is checked against docs/platforms.md (`platforms_doc_lists_the_claude_pins`);
    // the hash values are checked by review only.
    let unsupported = |os, arch| {
        let error = falinks_claude::pin(os, arch).unwrap_err().to_string();
        assert!(error.contains("unsupported platform"), "{error}");
    };
    for &(os, arch) in falinks_host::PLATFORMS {
        if !falinks_claude::PINS
            .iter()
            .any(|(platform, _)| *platform == (os, arch))
        {
            unsupported(os, arch);
        }
    }
    // Windows has no docs column, so `platforms_row` forbids pinning it: a fixed sentinel.
    unsupported("windows", "x86_64");
}

#[test]
fn platforms_doc_lists_the_claude_pins() {
    let doc = include_str!("../../../docs/platforms.md");
    let row = falinks_host::platforms_row("Claude Code adapter", falinks_claude::PINS);
    assert!(doc.lines().any(|line| line == row), "missing row: {row}");
}

/// Check parsing is `falinks_host::probe_controls`; this adapter only finds the exact
/// command's successful Bash result.
#[test]
fn probe_controls_come_only_from_the_exact_bash_command() {
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, "protected fixture\n").unwrap();
    let args = falinks_host::probe_args(
        Path::new("probe"),
        Path::new("source.txt"),
        "hash",
        Path::new("scratch"),
        &[("controller", &secret)],
    );
    let command = shlex::try_join(args.iter().map(String::as_str)).unwrap();
    let controls: serde_json::Map<String, Value> = falinks_host::probe_names(&["controller"])
        .into_iter()
        .map(|n| (n, json!(true)))
        .collect();
    let events = |ran: &str, is_error: bool| {
        [
            json!({"type":"assistant","message":{"content":[
                {"type":"tool_use","id":"t1","name":"Bash","input":{"command":ran}}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","is_error":is_error}]},
                "tool_use_result":{"stdout":format!("FALINKS_CONTROLS={}\n", Value::Object(controls.clone()))}}),
        ]
    };
    assert!(falinks_claude::bash_probe_controls(&events(&command, false), &command).is_ok());
    assert!(falinks_claude::bash_probe_controls(&events(&command, true), &command).is_err());
    let other = format!("{command} extra");
    assert!(falinks_claude::bash_probe_controls(&events(&other, false), &command).is_err());
}

#[test]
fn a_model_refusal_is_detected() {
    assert!(falinks_claude::refused(&[
        json!({"type":"system","subtype":"model_refusal_fallback"})
    ]));
    assert!(!falinks_claude::refused(&[
        json!({"type":"system","subtype":"init"})
    ]));
}

/// A session whose engine defers `check` to `slow` work and answers `edit` and `post` now,
/// with a stage report for `edit`.
fn deferring(slow: fn() -> falinks_host::Result<Value>) -> Fixture {
    let mut f = fixture();
    let mut launch = Launch::fixture(f.workspace.parent().unwrap());
    launch.tools =
        json!([{"name":"falinks_check"},{"name":"falinks_edit"},{"name":"falinks_post"}]);
    let engine = Box::new(move |envelope: &Value| {
        Ok(match envelope["operation"].as_str() {
            Some("check") => EngineOutcome {
                deferred: Some(falinks_host::Deferred {
                    work: Box::new(slow),
                    finish: Box::new(|checks| {
                        let checks = checks?;
                        falinks_host::require(checks["passed"].is_boolean(), "bad oracle line")?;
                        Ok(json!({"accepted":true,"passed":checks["passed"]}))
                    }),
                }),
                ..Default::default()
            },
            Some("edit") => EngineOutcome {
                result: json!({"accepted":true}),
                stages: Some(json!([{"stage":"LockWait","seconds":0.0}])),
                ..Default::default()
            },
            _ => EngineOutcome {
                result: json!({"accepted":true}),
                ..Default::default()
            },
        })
    });
    f.session = Session::new(&launch, "agent-a", SESSION, TOKEN, engine, false).unwrap();
    let mut line = init(&f.workspace);
    line["tools"] = json!(launch.allowed());
    f.session.line(&line).unwrap();
    f
}
fn serve(f: &mut Fixture, id: &str, tool: &str) -> falinks_claude::Served {
    let input = json!({});
    f.session.host(&hook("P1", id, tool, &input));
    f.session
        .serve(&call(id, tool, &input), std::time::Instant::now())
}
fn timed(record: &Value) -> bool {
    let at = |k: &str| record[k].as_f64().unwrap();
    at("received") <= at("started") && at("started") <= at("finished")
}

#[test]
fn a_slow_check_runs_off_the_driver_and_the_peer_posts_meanwhile() {
    use falinks_claude::Served;
    use std::{sync::mpsc, thread, time::Duration, time::Instant};
    let slow = || {
        thread::sleep(Duration::from_millis(500));
        Ok(json!({"passed":true}))
    };
    let (mut mine, mut peer) = (deferring(slow), deferring(slow));
    let started = Instant::now();
    let Served::Later(key, work) = serve(&mut mine, "toolu_1", "falinks_check") else {
        panic!("check answered on the driver");
    };
    let (done, finished) = mpsc::channel();
    thread::spawn(move || done.send(work()));
    let Served::Now(reply, _) = serve(&mut peer, "toolu_2", "falinks_post") else {
        panic!("post deferred");
    };
    assert_eq!(reply["accepted"], true);
    assert!(started.elapsed() < Duration::from_millis(400));
    assert!(
        finished.try_recv().is_err(),
        "the check finished before the peer's post"
    );
    let answer = mine.session.finish(key, finished.recv().unwrap());
    assert_eq!(answer, json!({"accepted":true,"passed":true}));
    for f in [&mine, &peer] {
        let record = f.session.events.last().unwrap();
        assert!(record["host_call"].is_object() && timed(record), "{record}");
    }
    assert!(
        mine.session.events.last().unwrap()["finished"]
            .as_f64()
            .unwrap()
            >= 0.5
    );
    assert!(!mine.session.gate.failed);
}

#[test]
fn every_host_call_record_is_timed_and_an_edit_holds_its_stage_report() {
    let mut f = deferring(|| Ok(json!({"passed":false})));
    let input = json!({});
    for (id, tool) in [
        ("toolu_1", "falinks_edit"),
        ("toolu_2", "falinks_post"),
        ("toolu_3", "falinks_check"),
    ] {
        f.session.host(&hook("P1", id, tool, &input));
        let reply = f.session.host(&call(id, tool, &input)).0;
        // Agents never see the stage report or the times.
        assert!(reply.get("stages").is_none() && reply.get("received").is_none());
    }
    let records: Vec<&Value> = f
        .session
        .events
        .iter()
        .filter(|e| e["host_call"].is_object())
        .collect();
    assert_eq!(records.len(), 3);
    assert!(records.iter().all(|r| timed(r)));
    assert_eq!(
        records[0]["stages"],
        json!([{"stage":"LockWait","seconds":0.0}])
    );
    assert!(records[1].get("stages").is_none());
    assert_eq!(
        records[2]["result"],
        json!({"accepted":true,"passed":false})
    );
}

#[test]
fn a_check_that_errors_still_answers_its_caller_and_latches_the_gate() {
    for slow in [
        (|| Err("oracle failed: boom".into())) as fn() -> falinks_host::Result<Value>,
        || Ok(json!({"garbled":true})),
    ] {
        let mut f = deferring(slow);
        let input = json!({});
        f.session
            .host(&hook("P1", "toolu_1", "falinks_check", &input));
        let reply = f.session.host(&call("toolu_1", "falinks_check", &input)).0;
        assert_eq!(reply["accepted"], false, "{reply}");
        assert!(f.session.gate.failed);
        assert!(timed(f.session.events.last().unwrap()));
    }
}
