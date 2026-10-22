//! The production engine behind authenticated tool envelopes (#26): scripted calls
//! against a real temporary engine, Git and SQLite, with the pinned gopls/Go analysis.
use falinks::{Check, Client, Engine, Language, Pin, Toolchain};
use falinks_claude::engine_host::EngineHost;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};

const GO: &[(&str, &str)] = &[
    ("go.mod", "module example.com/gate\n\ngo 1.22\n"),
    (
        "price.go",
        "package gate\n\nfunc Net(gross int) int {\n\treturn gross - Discount(gross)\n}\n\nfunc Discount(gross int) int {\n\treturn 0\n}\n",
    ),
    (
        "label.go",
        "package gate\n\nfunc Label(name string) string {\n\treturn name\n}\n",
    ),
];

struct Fixture {
    _dir: tempfile::TempDir,
    live: PathBuf,
    engine: Arc<Engine>,
    clients: [Client; 2],
    hosts: [EngineHost; 2],
}
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let live = root.join("live");
    for (path, bytes) in GO {
        fs::create_dir_all(live.join(path).parent().unwrap()).unwrap();
        fs::write(live.join(path), bytes).unwrap();
    }
    let enrolled: Vec<_> = GO.iter().map(|(p, _)| *p).collect();
    let engine = Arc::new(Engine::open(&live, &root.join("state"), &enrolled).unwrap());
    let (go, gopls) = falinks_claude::go_tools().unwrap();
    engine
        .configure_analysis(vec![Toolchain {
            language: Language::Go,
            executables: vec![Pin::from_path(gopls).unwrap(), Pin::from_path(&go).unwrap()],
            features: Vec::new(),
        }])
        .unwrap();
    engine
        .configure_checks(vec![Check {
            name: "trusted".into(),
            program: "/usr/bin/true".into(),
            args: Vec::new(),
        }])
        .unwrap();
    let clients = [0, 1].map(|a| engine.authenticate(a, &engine.credentials()[a]).unwrap());
    let host = |agent: usize| {
        EngineHost::new(
            Arc::clone(&engine),
            clients[agent].clone(),
            &format!("agent-{agent}"),
            go.with_file_name("gofmt"),
        )
    };
    Fixture {
        hosts: [host(0), host(1)],
        clients,
        engine: Arc::clone(&engine),
        _dir: dir,
        live,
    }
}
/// The envelope `Boundary::authenticate` produces for a stamped call.
fn envelope(agent: usize, workspace: &std::path::Path, operation: &str, request: Value) -> Value {
    json!({"identity":{"agent":format!("agent-{agent}"),"session":"connection","thread":"thread",
        "turn":"turn","workspace":workspace},"operation":operation,"request":request})
}
impl Fixture {
    fn call(&self, agent: usize, operation: &str, request: Value) -> Value {
        self.hosts[agent]
            .call(&envelope(agent, &self.live, operation, request))
            .unwrap()
    }
}

#[test]
fn mediated_edits_carry_host_identity_and_stale_requests_apply_nothing() {
    let f = fixture();
    let before = f.call(0, "capture", json!({"paths":["price.go"]}));
    assert_eq!(before["accepted"], true, "{before}");
    let version = before["files"]["price.go"]["version"].clone();
    assert_eq!(before["files"]["price.go"]["content"], GO[1].1);
    // Incomplete code is accepted as a live draft.
    let draft = "package gate\n\nfunc Discount(gross int) int {\n\treturn gross /\n";
    let applied = f.call(
        0,
        "edit",
        json!({"id":"draft","expected":{"price.go":version},"output":{"price.go":{"content":draft}}}),
    );
    assert_eq!(applied["accepted"], true, "{applied}");
    assert_eq!(fs::read_to_string(f.live.join("price.go")).unwrap(), draft);

    // The peer's two-file request from the old capture is stale as a whole.
    let label = before["files"]["label.go"]["version"].clone();
    let stale = f.call(
        1,
        "edit",
        json!({"id":"both","expected":{"price.go":version,"label.go":label},
            "output":{"price.go":{"content":"package gate\n"},"label.go":{"content":"package gate\n"}}}),
    );
    assert_eq!(stale["accepted"], false, "{stale}");
    assert_eq!(
        stale["outcome"]["Stale"]["conflicts"]["price.go"]["current"]["file"]["content"],
        draft
    );
    assert_eq!(
        fs::read_to_string(f.live.join("label.go")).unwrap(),
        GO[2].1
    );
    // The whole proposal is retained under the peer's request ID.
    let retained = f.call(1, "request", json!({"id":"both"}));
    assert_eq!(retained["record"]["agent"], 1, "{retained}");
    assert_eq!(
        retained["record"]["request"]["output"]["label.go"]["content"],
        "package gate\n"
    );
}

const DISCOUNT: &str = "package gate\n\nfunc Net(gross int) int {\n\treturn gross - Discount(gross)\n}\n\nfunc Discount(gross int) int {\n\treturn gross / 10\n}\n";
const NET: &str = "package gate\n\nfunc Net(gross int) int {\n\treturn max(0, gross-Discount(gross))\n}\n\nfunc Discount(gross int) int {\n\treturn gross / 10\n}\n";

#[test]
fn relevant_peer_edits_gate_writes_and_offers_until_an_engine_review() {
    let f = fixture();
    for (agent, id, node) in [
        (0, "discount", "price.go#Discount"),
        (1, "net", "price.go#Net"),
    ] {
        let registered = f.call(agent, "register", json!({"id":id,"task":id,"nodes":[node]}));
        assert_eq!(registered["accepted"], true, "{registered}");
    }
    let version = f.call(0, "capture", json!({}))["files"]["price.go"]["version"].clone();
    let applied = f.call(
        0,
        "edit",
        json!({"id":"d1","expected":{"price.go":version},"output":{"price.go":{"content":DISCOUNT}}}),
    );
    assert_eq!(applied["accepted"], true, "{applied}");

    // Agent 1 is told through replayable events; its obligation is pending.
    let pending = f.engine.pending(&f.clients[1]).unwrap();
    assert_eq!(pending.obligations[0].scope.id, "net", "{pending:?}");
    let seq = pending.unhandled[0].seq;
    let deferred = f.call(
        1,
        "handle",
        json!({"through":seq,"deferred":{seq.to_string():"after my edit"}}),
    );
    assert_eq!(deferred["accepted"], true, "{deferred}");
    let pending = f.engine.pending(&f.clients[1]).unwrap();
    assert_eq!(pending.deferred[0].1, "after my edit", "{pending:?}");
    assert_eq!(pending.obligations.len(), 1);

    // Handling or deferring is not a review: related writes and offers stay closed.
    let current = f.call(1, "capture", json!({}));
    let (revision, version) = (
        current["revision"].clone(),
        current["files"]["price.go"]["version"].clone(),
    );
    let edit =
        json!({"id":"n1","expected":{"price.go":version},"output":{"price.go":{"content":NET}}});
    let blocked = f.call(1, "edit", edit.clone());
    assert!(blocked["outcome"]["Unreviewed"].is_object(), "{blocked}");
    let offer = json!({"id":"o1","task":"net","revision":revision,"scope":["net"],"text":"net"});
    assert_eq!(f.call(1, "offer", offer.clone())["accepted"], false);

    let reviewed = f.call(
        1,
        "review",
        json!({"scope":"net","revision":revision,"decision":"Keep","note":"reread Discount"}),
    );
    assert_eq!(reviewed["accepted"], true, "{reviewed}");
    let processed = f.call(1, "handle", json!({"processed":[seq]}));
    assert_eq!(processed["accepted"], true, "{processed}");
    assert!(f.engine.pending(&f.clients[1]).unwrap().deferred.is_empty());
    let applied = f.call(
        1,
        "edit",
        json!({"id":"n2","expected":edit["expected"],"output":edit["output"]}),
    );
    assert_eq!(applied["accepted"], true, "{applied}");
    // Agent 0 now owes a review of agent 1's change before it may offer.
    let obligations = f.call(0, "obligations", json!({}));
    assert_eq!(
        obligations["obligations"][0]["scope"]["id"], "discount",
        "{obligations}"
    );
    // An inline review on the edit itself clears it in the same call.
    let current = f.call(0, "capture", json!({}));
    let inline = f.call(
        0,
        "edit",
        json!({"id":"d2","expected":{"price.go":current["files"]["price.go"]["version"]},
            "output":{"price.go":{"content":DISCOUNT}},
            "reviews":[{"scope":"discount","revision":current["revision"],
                "decision":"Keep","note":"reread Net"}]}),
    );
    assert_eq!(inline["accepted"], true, "{inline}");
}

#[test]
fn exact_offers_publish_the_checked_candidate_and_feedback_never_publishes() {
    let f = fixture();
    let version = f.call(0, "capture", json!({}))["files"]["price.go"]["version"].clone();
    let applied = f.call(
        0,
        "edit",
        json!({"id":"d1","expected":{"price.go":version},"output":{"price.go":{"content":DISCOUNT}}}),
    );
    let revision = applied["outcome"]["Applied"]["revision"].clone();
    // Feedback, offers and reviews name a revision the agent captured.
    assert_eq!(f.call(0, "capture", json!({}))["revision"], revision);
    let posted = f.call(
        0,
        "post",
        json!({"task":"discount","scope":[],"work":{"Revision":revision},"text":"draft ready","reply_to":null}),
    );
    assert_eq!(posted["accepted"], true, "{posted}");

    let feedback = f.call(0, "feedback", json!({"id":"early","revision":revision}));
    assert_eq!(feedback["accepted"], true, "{feedback}");
    assert!(f.engine.validate().unwrap().is_some());
    let run = f.call(0, "run", json!({"id":feedback["run"]["id"]}));
    assert_eq!(run["run"]["outcome"], "Passed", "{run}");
    assert_eq!(f.call(0, "capture", json!({}))["revision"], revision);

    // Only agent 0 contributed: its exact offer completes coverage.
    let candidate = f.call(1, "candidate", json!({"revision":revision}));
    assert_eq!(candidate["missing"], json!([0]), "{candidate}");
    let offered = f.call(
        0,
        "offer",
        json!({"id":"o1","task":"discount","revision":revision,"scope":[],"text":"discount"}),
    );
    assert_eq!(offered["accepted"], true, "{offered}");
    assert!(f.engine.validate().unwrap().is_some());
    let checkpoint = f.call(1, "checkpoint", json!({"id":"0/o1"}));
    assert!(
        checkpoint["checkpoint"]["state"]["Accepted"].is_object(),
        "{checkpoint}"
    );
    let withdrawn = f.call(0, "withdraw", json!({"offer":"o1"}));
    assert_eq!(
        withdrawn["accepted"], false,
        "accepted work cannot be withdrawn: {withdrawn}"
    );
}

#[test]
fn agreed_split_moves_the_agent_and_its_old_launch_loses_engine_access() {
    let f = fixture();
    f.call(
        0,
        "register",
        json!({"id":"discount","task":"discount","nodes":["price.go#Discount"]}),
    );
    f.call(
        1,
        "register",
        json!({"id":"label","task":"label","nodes":["label.go#Label"]}),
    );
    let second = f.live.with_file_name("second");
    fs::create_dir(&second).unwrap();
    f.engine.add_workspace(&second).unwrap();

    let proposal = f.call(1, "recommend", json!({}));
    assert_eq!(proposal["proposal"]["action"], "Split", "{proposal}");
    let (id, context) = (
        proposal["proposal"]["id"].clone(),
        proposal["proposal"]["context"].clone(),
    );
    for agent in [0, 1] {
        let agreed = f.call(
            agent,
            "respond",
            json!({"id":id,"context":context,"response":"Agree"}),
        );
        assert_eq!(agreed["accepted"], true, "{agreed}");
    }
    assert_eq!(f.engine.placement().unwrap(), [0, 1]);

    // The old launch's workspace no longer matches: a host error that latches the worker.
    let stale = f.hosts[1].call(&envelope(1, &f.live, "capture", json!({})));
    assert!(stale.is_err());
    let moved = f.hosts[1]
        .call(&envelope(1, &second, "capture", json!({})))
        .unwrap();
    assert_eq!(moved["space"], 1, "{moved}");

    let proposed = f.call(
        0,
        "propose",
        json!({"action":"Join","text":"reverting","failing":false}),
    );
    assert_eq!(
        proposed["proposal"]["action"], "Keep",
        "no reversal without new evidence: {proposed}"
    );
}

#[test]
fn the_enrolled_formatter_runs_as_a_captured_job_attributed_to_its_invoker() {
    let f = fixture();
    let version = f.call(1, "capture", json!({}))["files"]["label.go"]["version"].clone();
    let messy = "package gate\n\nfunc   Label(name string) string { return \"item: \"+name }\n";
    let applied = f.call(
        1,
        "edit",
        json!({"id":"l1","expected":{"label.go":version},"output":{"label.go":{"content":messy}}}),
    );
    let revision = applied["outcome"]["Applied"]["revision"].clone();
    assert_eq!(f.call(1, "capture", json!({}))["revision"], revision);
    // Agents name paths only; the host enrolls the program and its arguments.
    let job = f.call(
        1,
        "job",
        json!({"id":"fmt","revision":revision,"paths":["label.go"],"program":"/bin/sh"}),
    );
    assert_eq!(
        job["accepted"], false,
        "agents cannot choose the program: {job}"
    );
    let job = f.call(
        1,
        "job",
        json!({"id":"fmt","revision":revision,"paths":["label.go"]}),
    );
    assert_eq!(job["accepted"], true, "{job}");
    let applied = f.call(1, "apply_job", json!({"id":"fmt"}));
    assert_eq!(applied["accepted"], true, "{applied}");
    assert_eq!(
        fs::read_to_string(f.live.join("label.go")).unwrap(),
        "package gate\n\nfunc Label(name string) string { return \"item: \" + name }\n"
    );
    let record = f.call(1, "request", json!({"id":"job:fmt"}));
    assert_eq!(record["record"]["agent"], 1, "{record}");
}

#[test]
fn foreign_identity_and_unknown_source_changes_are_host_errors() {
    let f = fixture();
    assert!(
        f.hosts[0]
            .call(&envelope(1, &f.live, "capture", json!({})))
            .is_err()
    );
    assert_eq!(f.call(0, "nonsense", json!({}))["accepted"], false);
    let version = f.call(0, "capture", json!({}))["files"]["label.go"]["version"].clone();
    fs::write(f.live.join("label.go"), "UNEXPECTED\n").unwrap();
    let error = f.hosts[0]
        .call(&envelope(
            0,
            &f.live,
            "edit",
            json!({"id":"x","expected":{"label.go":version},"output":{"label.go":{"content":"package gate\n"}}}),
        ))
        .unwrap_err();
    assert!(error.to_string().contains("incident"), "{error}");
    // Evidence is preserved, never overwritten.
    assert_eq!(
        fs::read_to_string(f.live.join("label.go")).unwrap(),
        "UNEXPECTED\n"
    );
}

#[test]
fn every_advertised_tool_maps_to_an_engine_operation() {
    let f = fixture();
    let tools = falinks_claude::engine_host::tools();
    for tool in tools.as_array().unwrap() {
        let operation = tool["name"]
            .as_str()
            .unwrap()
            .strip_prefix("falinks_")
            .unwrap();
        let reply = f.call(0, operation, json!({}));
        let reason = reply["reason"].as_str().unwrap_or("");
        assert!(
            !reason.starts_with("unsupported operation"),
            "{operation}: {reply}"
        );
    }
}

/// Notices and the opening prompt carry what agents need; hosts keep `pending` and `events`.
#[test]
fn agents_get_no_pending_or_events_tool() {
    let f = fixture();
    let tools = falinks_claude::engine_host::tools();
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"falinks_handle"), "{names:?}");
    for hidden in ["falinks_pending", "falinks_events"] {
        assert!(!names.contains(&hidden), "{names:?}");
    }
    for operation in ["pending", "events"] {
        let reply = f.call(0, operation, json!({}));
        assert_eq!(reply["accepted"], false, "{reply}");
    }
}

/// A notice whose events renew the recipient's obligation carries a capture given to it, so
/// the agent reviews inline on its next edit without a capture call.
#[test]
fn a_renewing_notice_capture_authorizes_an_inline_review() {
    use falinks_claude::engine_host::{notice_capture, opening};
    let f = fixture();
    for (agent, id, node) in [
        (0, "discount", "price.go#Discount"),
        (1, "net", "price.go#Net"),
    ] {
        let registered = f.call(agent, "register", json!({"id":id,"task":id,"nodes":[node]}));
        assert_eq!(registered["accepted"], true, "{registered}");
    }
    let (start, mut given) = opening(&f.engine, &f.clients[1]).unwrap();
    let version = start["files"]["price.go"]["version"].clone();
    assert!(
        start["files"]["price.go"].get("content").is_none(),
        "{start}"
    );
    // Events that renew nothing carry no capture.
    let quiet = f.engine.events(&f.clients[1], 0).unwrap();
    assert_eq!(
        notice_capture(&f.engine, &f.clients[1], &quiet, &mut given).unwrap(),
        None
    );

    let applied = f.call(
        0,
        "edit",
        json!({"id":"d1","expected":{"price.go":version},"output":{"price.go":{"content":DISCOUNT}}}),
    );
    assert_eq!(applied["accepted"], true, "{applied}");
    let events = f.engine.events(&f.clients[1], 0).unwrap();
    let notice = notice_capture(&f.engine, &f.clients[1], &events, &mut given)
        .unwrap()
        .expect("renewing events carry a capture");
    // Only the changed file's content travels with the notice.
    assert_eq!(notice["files"]["price.go"]["content"], DISCOUNT, "{notice}");
    assert!(
        notice["files"]["label.go"].get("content").is_none(),
        "{notice}"
    );

    let inline = f.call(
        1,
        "edit",
        json!({"id":"n1","expected":{"price.go":notice["files"]["price.go"]["version"]},
            "output":{"price.go":{"content":NET}},
            "reviews":[{"scope":"net","revision":notice["revision"],
                "decision":"Keep","note":"reread Discount in the notice"}]}),
    );
    assert_eq!(inline["accepted"], true, "{inline}");
}

/// An edit output value is `{content, executable?}` or null. A mixed or unknown form is refused
/// before the engine sees it, so the file is untouched and the request ID stays free.
#[test]
fn edit_output_values_are_whole_text_or_null_and_other_forms_are_refused() {
    let f = fixture();
    let edit = |id: &str, value: Value| {
        let version = f.call(0, "capture", json!({}))["files"]["label.go"]["version"].clone();
        f.call(
            0,
            "edit",
            json!({"id":id,"expected":{"label.go":version},"output":{"label.go":value}}),
        )
    };
    for value in [
        json!({"content":"package gate\n","replace":[{"old":"gate","new":"other"}]}),
        json!({"content":"package gate\n","from":"label.go"}),
        json!({"content":"package gate\n","mode":"0755"}),
    ] {
        let refused = edit("x", value.clone());
        assert_eq!(refused["accepted"], false, "{value}: {refused}");
        let reason = refused["reason"].as_str().unwrap();
        assert!(reason.starts_with("request keys:"), "{reason}");
        assert!(reason.contains("content, executable?"), "{reason}");
        assert_eq!(
            fs::read_to_string(f.live.join("label.go")).unwrap(),
            GO[2].1
        );
    }
    let whole = edit("x", json!({"content":"package gate\n"}));
    assert_eq!(whole["accepted"], true, "{whole}");
    assert_eq!(
        fs::read_to_string(f.live.join("label.go")).unwrap(),
        "package gate\n"
    );
    let deleted = edit("y", Value::Null);
    assert_eq!(deleted["accepted"], true, "{deleted}");
    assert!(!f.live.join("label.go").exists());
}

/// Every operation checks its request keys, top level and nested, before the engine sees them,
/// naming the keys it takes. A request wrapped in `request` is refused rather than read as empty.
#[test]
fn wrapped_unknown_or_missing_request_keys_are_refused_by_name() {
    let f = fixture();
    let version = f.call(0, "capture", json!({}))["files"]["label.go"]["version"].clone();
    let output = json!({"label.go":{"content":"package gate\n"}});
    for (operation, request, takes) in [
        (
            "handle",
            json!({"request":{"through":5}}),
            "handle takes {through?, deferred?, processed?}",
        ),
        ("capture", json!({"request":{}}), "capture takes {paths?}"),
        ("obligations", json!({"all":true}), "obligations takes {}"),
        (
            "edit",
            json!({"id":"e","expected":{"label.go":version},"output":output,"request":{}}),
            "edit takes {id, expected, output, reviews?}",
        ),
        (
            "edit",
            json!({"id":"e","expected":{"label.go":version},"output":output,
            "reviews":[{"scope":"s","revision":0,"decision":"Keep"}]}),
            "reviews[] takes {scope, revision, decision, note}",
        ),
        (
            "register",
            json!({"id":"r","task":"t"}),
            "register takes {id, task, nodes, depends?}",
        ),
        (
            "post",
            json!({"task":"t","scope":[],"work":"None","text":"hi","reply_to":{"event":1}}),
            "reply_to takes {event, work}",
        ),
        (
            "respond",
            json!({"id":"p","context":"c","response":{"Decline":{"reason":"no","why":"x"}}}),
            "response.Decline takes {reason}",
        ),
        (
            "job",
            json!({"id":"j","revision":0}),
            "job takes {id, revision, paths}",
        ),
    ] {
        let reply = f.call(0, operation, request.clone());
        let reason = reply["reason"].as_str().unwrap_or_default();
        assert!(
            reason.starts_with(&format!("request keys: {takes}")),
            "{operation} {request}: {reply}"
        );
    }
    // The refused edits used up no request ID.
    let edit = f.call(
        0,
        "edit",
        json!({"id":"e","expected":{"label.go":version},"output":output}),
    );
    assert_eq!(edit["accepted"], true, "{edit}");
    // An optional key keeps its default.
    let post = f.call(
        0,
        "post",
        json!({"task":"t","scope":[],"work":"None","text":"hi"}),
    );
    assert_eq!(post["accepted"], true, "{post}");
    assert_eq!(
        post["event"]["body"]["Message"]["reply_to"],
        Value::Null,
        "{post}"
    );
}

/// Each tool's input is the request itself: no definition names the run path, and no input
/// schema has the old `{workspace, request}` wrapper.
#[test]
fn tool_inputs_are_the_request_and_never_name_the_run_path() {
    let f = fixture();
    let tools = falinks_claude::arm::tools();
    for tool in tools.as_array().unwrap() {
        assert_eq!(tool["inputSchema"], json!({"type":"object"}), "{tool}");
        let text = tool.to_string();
        assert!(!text.contains(f.live.to_str().unwrap()), "{text}");
        assert!(!text.contains("Call with"), "{text}");
    }
}
