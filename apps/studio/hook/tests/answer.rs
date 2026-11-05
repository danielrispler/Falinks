// Seam 4 (#461): the answer hook's contract. The binary gets a hook payload on stdin, a scratch `HOME`
// in which the test plays the studio (its watching file and its answer file), and is checked on
// stdout.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use studio_hook::answer;

const SESSION: &str = "s-1";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn scratch(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("studio-answer-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    home
}

fn data(home: &Path) -> PathBuf {
    studio_hook::event_log(home).parent().unwrap().to_owned()
}

/// The studio watches `session`: its pid and a heartbeat `age_ms` old.
fn watch(home: &Path, session: &str, pid: u32, age_ms: u64) {
    let watching = json!({ "session_id": session, "pid": pid, "heartbeat_ms": now_ms() - age_ms });
    answer::write_private(&answer::watching_file(&data(home)), &watching).unwrap();
}

fn asked() -> Value {
    json!({
        "session_id": SESSION,
        "hook_event_name": "PreToolUse",
        "tool_name": "AskUserQuestion",
        "tool_use_id": "toolu_1",
        "tool_input": { "questions": [
            { "question": "Which framework?", "header": "Framework", "multiSelect": false,
              "options": [{ "label": "React", "description": "Component library" }, { "label": "Vue" }] },
            { "question": "Which checks?", "header": "Checks", "multiSelect": true,
              "options": [{ "label": "Lint" }, { "label": "Tests" }, { "label": "Docs" }] }
        ] }
    })
}

fn start(home: &Path, timeout_ms: u64, payload: &Value) -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_studio-answer"))
        .env("HOME", home)
        .env("STUDIO_ANSWER_TIMEOUT_MS", timeout_ms.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(payload.to_string().as_bytes()).unwrap();
    child
}

/// What the hook printed, and how long it took from `since`.
fn finish(child: Child, since: Instant) -> (String, Duration) {
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    (String::from_utf8(out.stdout).unwrap(), since.elapsed())
}

/// The request the hook leaves for the studio, once it appears.
fn request(home: &Path) -> Value {
    let path = answer::ask_file(&data(home), SESSION);
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if let Ok(text) = fs::read_to_string(&path) {
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            return serde_json::from_str(&text).unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("no request appeared");
}

fn reply(home: &Path, answer: Value) {
    answer::write_private(&answer::answer_file(&data(home), SESSION), &answer).unwrap();
}

#[test]
fn an_answer_allows_the_call_with_the_questions_and_each_chosen_label() {
    let home = scratch("answer");
    watch(&home, SESSION, std::process::id(), 0);
    let since = Instant::now();
    let child = start(&home, 10_000, &asked());
    let ask = request(&home);
    assert_eq!(ask["tool_input"], asked()["tool_input"]);
    let answers = json!({ "Which framework?": "React", "Which checks?": ["Lint", "Tests"] });
    reply(&home, json!({ "nonce": ask["nonce"], "answers": answers }));
    let (out, _) = finish(child, since);
    let out: Value = serde_json::from_str(&out).unwrap();
    let mut input = asked()["tool_input"].clone();
    input["answers"] = json!({ "Which framework?": "React", "Which checks?": "Lint, Tests" });
    assert_eq!(
        out,
        json!({ "hookSpecificOutput": {
            "hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": input } })
    );
    // The request is gone once answered.
    assert!(!answer::ask_file(&data(&home), SESSION).exists());
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn nobody_watching_or_a_stale_watching_file_returns_at_once_with_no_decision() {
    let home = scratch("stale");
    let gone = Command::new("true").spawn().unwrap();
    let dead = gone.id();
    gone.wait_with_output().unwrap();
    let me = std::process::id();
    // Each case's watching file: the session watched, the pid and the heartbeat's age.
    type Watching<'a> = Option<(&'a str, u32, u64)>;
    let cases: [(&str, Watching); 4] = [
        ("no watching file", None),
        ("another session watched", Some(("s-2", me, 0))),
        ("a dead pid", Some((SESSION, dead, 0))),
        ("an old heartbeat", Some((SESSION, me, 6_000))),
    ];
    for (case, watching) in cases {
        let _ = fs::remove_file(answer::watching_file(&data(&home)));
        if let Some((session, pid, age)) = watching {
            watch(&home, session, pid, age);
        }
        let since = Instant::now();
        let (out, took) = finish(start(&home, 10_000, &asked()), since);
        assert_eq!(out, "", "{case}");
        assert!(took < Duration::from_secs(2), "{case}: took {took:?}");
        assert!(!answer::ask_file(&data(&home), SESSION).exists(), "{case}");
    }
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn a_timeout_or_answer_in_the_terminal_returns_no_decision() {
    let home = scratch("terminal");
    watch(&home, SESSION, std::process::id(), 0);

    let since = Instant::now();
    let (out, took) = finish(start(&home, 600, &asked()), since);
    assert_eq!(out, "");
    assert!(
        took >= Duration::from_millis(600),
        "returned before its timeout: {took:?}"
    );

    let since = Instant::now();
    let child = start(&home, 10_000, &asked());
    let ask = request(&home);
    reply(&home, json!({ "nonce": ask["nonce"] }));
    let (out, took) = finish(child, since);
    assert_eq!(out, "");
    assert!(took < Duration::from_secs(5));
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn an_answer_with_a_wrong_nonce_or_open_to_others_is_refused() {
    let home = scratch("nonce");
    watch(&home, SESSION, std::process::id(), 0);
    let answers = json!({ "Which framework?": "Vue" });

    let since = Instant::now();
    let child = start(&home, 1_500, &asked());
    let ask = request(&home);
    assert_ne!(ask["nonce"], "");
    reply(
        &home,
        json!({ "nonce": "0123456789abcdef0123456789abcdef", "answers": answers }),
    );
    assert_eq!(finish(child, since).0, "");

    // The right nonce in a file others may write is refused too.
    let since = Instant::now();
    let child = start(&home, 1_500, &asked());
    let ask = request(&home);
    let path = answer::answer_file(&data(&home), SESSION);
    let temp = path.with_extension("open");
    fs::write(
        &temp,
        json!({ "nonce": ask["nonce"], "answers": answers }).to_string(),
    )
    .unwrap();
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o644)).unwrap();
    fs::rename(&temp, &path).unwrap();
    assert_eq!(finish(child, since).0, "");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn other_tools_pass_through_at_once() {
    let home = scratch("other");
    watch(&home, SESSION, std::process::id(), 0);
    let mut payload = asked();
    payload["tool_name"] = "Bash".into();
    let since = Instant::now();
    let (out, took) = finish(start(&home, 10_000, &payload), since);
    assert_eq!(out, "");
    assert!(took < Duration::from_secs(2));
    let _ = fs::remove_dir_all(&home);
}
