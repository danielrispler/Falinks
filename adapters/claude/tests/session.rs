use falinks_claude::{Launch, MODEL, Session};
use falinks_host::runtime::{EngineOutcome, Notice};
use serde_json::{Value, json};
use std::{cell::RefCell, path::Path, rc::Rc};

const SESSION: &str = "19a2d42a-a536-4dcb-9fb2-6de7a59d7699";
const TOKEN: &str = "host-token";

/// Shapes recorded from the pinned binary in #32 (`2026-10-10-probe.json`).
fn init(workspace: &Path) -> Value {
    json!({"type":"system","subtype":"init","claude_code_version":"2.1.287","cwd":workspace.with_file_name("scratch"),
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
fn edit(workspace: &Path) -> Value {
    json!({"workspace":workspace,"request":{"request_id":"edit-1","content":"new"}})
}

#[test]
fn stamped_call_in_a_verified_turn_reaches_engine_with_host_identity() {
    let mut f = fixture();
    let input = edit(&f.workspace);
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
    let input = edit(&f.workspace);
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
    changed["request"]["content"] = json!("swapped");
    assert!(rejected(
        f.session.host(&call("toolu_4", "falinks_edit", &changed)).0
    ));
    f.session
        .host(&hook("P1", "toolu_5", "falinks_edit", &input));
    assert!(rejected(
        f.session.host(&call("toolu_5", "falinks_offer", &input)).0
    ));
    // Caller-supplied identity and another workspace.
    for (index, arguments) in [
        json!({"workspace":f.workspace,"request":{"author":"agent-b"}}),
        json!({"workspace":"/unauthorized","request":{"content":"x"}}),
        json!({"workspace":f.workspace,"request":{"content":"x"},"session":"forged"}),
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
        let input = edit(&f.workspace);
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

fn replay(text: &str) -> Value {
    json!({"type":"user","isReplay":true,"message":{"role":"user","content":text}})
}

#[test]
fn delivery_is_presentation_only_and_records_its_turn() {
    let mut f = fixture_with(Some("E1"));
    let input = edit(&f.workspace);
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
    std::fs::write(&impostor, b"#!/bin/sh\necho '2.1.287 (Claude Code)'\n").unwrap();
    let error = falinks_claude::verify_binary(&impostor)
        .unwrap_err()
        .to_string();
    // An unpinned platform refuses before hashing; a pinned one rejects the hash.
    let expected = match falinks_claude::pin(std::env::consts::OS, std::env::consts::ARCH) {
        Ok(_) => "SHA-256",
        Err(_) => "unsupported platform",
    };
    assert!(error.contains(expected), "{error}");
    assert!(falinks_claude::verify_binary(&dir.path().join("missing")).is_err());
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
            })
        }),
        false,
    )
    .unwrap();
    let mut line = init(&f.workspace);
    line["tools"] = json!(launch.allowed());
    f.session.line(&line).unwrap();
    let input = json!({"workspace":f.workspace,"request":{}});
    f.session
        .host(&hook("P1", "toolu_1", "falinks_capture", &input));
    assert_eq!(
        f.session
            .host(&call("toolu_1", "falinks_capture", &input))
            .0,
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
    assert_eq!(
        falinks_claude::pin("macos", "aarch64").unwrap(),
        "6eab8333fe2121553100d8f40bfada384a3e989b94f947e18ba6677a6fcb41ea"
    );
    for (os, arch) in [
        ("macos", "x86_64"),
        ("linux", "x86_64"),
        ("windows", "x86_64"),
    ] {
        let error = falinks_claude::pin(os, arch).unwrap_err().to_string();
        assert!(error.contains("unsupported platform"), "{error}");
    }
}

#[test]
fn platforms_doc_lists_the_claude_pins() {
    let doc = include_str!("../../../docs/platforms.md");
    let row = falinks_host::platforms_row("Claude Code adapter", falinks_claude::PINS);
    assert!(doc.lines().any(|line| line == row), "missing row: {row}");
}
