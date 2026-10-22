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
        json!({"id": "draft", "note": "n","expected":{"price.go":version},"output":{"price.go":{"content":draft}}}),
    );
    assert_eq!(applied["accepted"], true, "{applied}");
    assert_eq!(fs::read_to_string(f.live.join("price.go")).unwrap(), draft);

    // The peer's two-file request from the old capture is stale as a whole.
    let label = before["files"]["label.go"]["version"].clone();
    let stale = f.call(
        1,
        "edit",
        json!({"id": "both", "note": "n","expected":{"price.go":version,"label.go":label},
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
        json!({"id": "d1", "note": "n","expected":{"price.go":version},"output":{"price.go":{"content":DISCOUNT}}}),
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
    let edit = json!({"id": "n1", "note": "n","expected":{"price.go":version},"output":{"price.go":{"content":NET}}});
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
        json!({"id": "n2", "note": "n","expected":edit["expected"],"output":edit["output"]}),
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
        json!({"id": "d2", "note": "n","expected":{"price.go":current["files"]["price.go"]["version"]},
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
        json!({"id": "d1", "note": "n","expected":{"price.go":version},"output":{"price.go":{"content":DISCOUNT}}}),
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
    // The offer result reports coverage as of its own commit.
    assert_eq!(offered["missing"], json!([]), "{offered}");
    assert_eq!(offered["queued"], true, "{offered}");
    assert!(offered["reason"].is_null(), "{offered}");
    assert!(f.engine.validate().unwrap().is_some());
    let keys = |reply: &Value| {
        reply
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    let coverage = [
        "accepted", "id", "missing", "queued", "reason", "revision", "scope", "state",
    ];
    assert_eq!(keys(&offered), coverage, "{offered}");
    assert_eq!(offered["queued"], true, "{offered}");
    let checkpoint = f.call(1, "checkpoint", json!({"id":"0/o1"}));
    assert_eq!(
        keys(&checkpoint),
        ["accepted", "id", "revision", "scope", "state"]
    );
    assert!(checkpoint["state"]["Accepted"].is_object(), "{checkpoint}");
    assert_eq!(checkpoint["revision"], revision, "{checkpoint}");
    let withdrawn = f.call(0, "withdraw", json!({"offer":"o1"}));
    assert_eq!(
        withdrawn["accepted"], false,
        "accepted work cannot be withdrawn: {withdrawn}"
    );
    assert_eq!(
        keys(&withdrawn),
        ["accepted", "reason"],
        "a refusal is unchanged"
    );
}

#[test]
fn an_offer_that_queues_nothing_shows_its_state_and_why() {
    let f = fixture();
    let capture = f.call(0, "capture", json!({}));
    let version = |c: &Value, path: &str| c["files"][path]["version"].clone();
    let applied = |agent, id: &str, path: &str, version: Value, content: &str| {
        let reply = f.call(
            agent,
            "edit",
            json!({"id":id,"expected":{path:version},
            "output":{path:{"content":content}}}),
        );
        assert_eq!(reply["accepted"], true, "{reply}");
        reply["outcome"]["Applied"]["revision"].clone()
    };
    applied(0, "d1", "price.go", version(&capture, "price.go"), DISCOUNT);
    let label = version(&f.call(1, "capture", json!({})), "label.go");
    let revision = applied(
        1,
        "l1",
        "label.go",
        label,
        "package gate\n\nfunc Label(name string) string {\n\treturn \"x\" + name\n}\n",
    );
    let offer =
        json!({"id":"o1","task":"discount","revision":revision,"scope":[],"text":"discount"});
    assert_eq!(f.call(0, "capture", json!({}))["revision"], revision);
    let offered = f.call(0, "offer", offer);
    let keys: Vec<_> = offered.as_object().unwrap().keys().cloned().collect();
    assert_eq!(
        keys,
        [
            "accepted", "id", "missing", "queued", "reason", "revision", "scope", "state"
        ]
    );
    assert_eq!(offered["missing"], json!([1]), "{offered}");
    assert_eq!(offered["queued"], false, "{offered}");
    assert!(!offered["reason"].is_null(), "{offered}");
    assert_eq!(offered["state"], "Available", "{offered}");

    let withdrawn = f.call(0, "withdraw", json!({"offer":"o1"}));
    let keys: Vec<_> = withdrawn.as_object().unwrap().keys().cloned().collect();
    assert_eq!(
        keys,
        ["accepted", "id", "revision", "scope", "state"],
        "{withdrawn}"
    );
    assert_eq!(withdrawn["state"], "Withdrawn", "{withdrawn}");
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
        json!({"id": "l1", "note": "n","expected":{"label.go":version},"output":{"label.go":{"content":messy}}}),
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
            json!({"id": "x", "note": "n","expected":{"label.go":version},"output":{"label.go":{"content":"package gate\n"}}}),
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
        json!({"id": "d1", "note": "n","expected":{"price.go":version},"output":{"price.go":{"content":DISCOUNT}}}),
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
        json!({"id": "n1", "note": "n","expected":{"price.go":notice["files"]["price.go"]["version"]},
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
            json!({"id": id, "note": "n","expected":{"label.go":version},"output":{"label.go":value}}),
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
            json!({"id": "e", "note": "n","expected":{"label.go":version},"output":output,"request":{}}),
            "edit takes {id, expected, output, note, reviews?}",
        ),
        (
            "edit",
            json!({"id": "e", "note": "n","expected":{"label.go":version},"output":output,
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
        json!({"id": "e", "note": "n","expected":{"label.go":version},"output":output}),
    );
    assert_eq!(edit["accepted"], true, "{edit}");
    // An optional key keeps its default.
    let post = f.call(
        0,
        "post",
        json!({"task":"t","scope":[],"work":"None","text":"hi"}),
    );
    assert_eq!(post["accepted"], true, "{post}");
    let event = f.engine.events(&f.clients[1], 0).unwrap().pop().unwrap();
    assert_eq!(post["seq"], event.seq, "{post}");
    let falinks::Body::Message(message) = event.body else {
        panic!("{event:?}")
    };
    assert_eq!(message.reply_to, None);
}

/// Whether `text` holds a run of exactly `len` hex digits.
fn has_hex(text: &str, len: usize) -> bool {
    text.split(|c: char| !c.is_ascii_hexdigit())
        .any(|run| run.len() == len)
}

/// `post` returns only its seq, notices show seqs with the workspace once in the header,
/// and edit and capture results carry no Git IDs (#221). The engine keeps its IDs.
#[test]
fn post_results_notices_and_edit_results_carry_no_unused_ids() {
    use falinks_claude::engine_host::{notice_events, notice_header};
    let f = fixture();
    let post = json!({"task":"t","scope":[],"work":"None","text":"plan"});
    let posted = f.call(0, "post", post);
    assert_eq!(
        posted,
        json!({"accepted":true,"seq":posted["seq"]}),
        "{posted}"
    );
    let seq = posted["seq"].as_u64().unwrap();
    // The agent handles through its own post's seq.
    let handled = f.call(0, "handle", json!({"through":seq}));
    assert_eq!(handled["accepted"], true, "{handled}");
    let reply = json!({"task":"t","scope":[],"work":"None","text":"ok",
        "reply_to":{"event":seq,"work":"None"}});
    let replied = f.call(1, "post", reply);
    assert_eq!(replied["accepted"], true, "{replied}");

    let events = f.engine.events(&f.clients[1], 0).unwrap();
    // Stored events keep `nonce:seq` IDs and the workspace.
    let nonce = events[0].id.split_once(':').unwrap().0.to_string();
    assert!(has_hex(&nonce, 64), "{nonce}");
    let falinks::Body::Message(message) = &events[0].body else {
        panic!("{events:?}")
    };
    assert_eq!(message.workspace, nonce);
    let shown = notice_events(&events).unwrap();
    assert_eq!(shown[0]["seq"], seq, "{shown}");
    assert!(shown[0].get("id").is_none(), "{shown}");
    assert!(!has_hex(&shown.to_string(), 64), "{shown}");
    let header = notice_header(&events).unwrap();
    assert_eq!(
        header,
        format!("{} (workspace {nonce})", events.last().unwrap().seq)
    );

    let before = f.call(0, "capture", json!({"paths":["label.go"]}));
    let applied = f.call(
        0,
        "edit",
        json!({"id":"e","expected":{"label.go":before["files"]["label.go"]["version"]},
            "output":{"label.go":{"content":"package gate\n"}}}),
    );
    assert_eq!(applied["accepted"], true, "{applied}");
    let stale = f.call(
        1,
        "edit",
        json!({"id":"s","expected":{"label.go":before["files"]["label.go"]["version"]},
            "output":{"label.go":null}}),
    );
    assert!(stale["outcome"]["Stale"].is_object(), "{stale}");
    let after = f.call(0, "capture", json!({"paths":["label.go"]}));
    for result in [&before, &applied, &stale, &after] {
        assert!(!has_hex(&result.to_string(), 40), "{result}");
    }
}

/// Each tool's input is the request itself: no definition names the run path, and no input
/// schema has the old `{workspace, request}` wrapper or requires a key, which the adapter
/// checks instead.
#[test]
fn tool_inputs_are_the_request_and_never_name_the_run_path() {
    let f = fixture();
    let tools = falinks_claude::arm::tools();
    for tool in tools.as_array().unwrap() {
        let schema = tool["inputSchema"].as_object().unwrap();
        assert_eq!(schema["type"], "object", "{tool}");
        assert_eq!(
            schema.keys().collect::<Vec<_>>(),
            ["properties", "type"],
            "{tool}"
        );
        for key in ["workspace", "request"] {
            assert!(schema["properties"].get(key).is_none(), "{tool}");
        }
        let text = tool.to_string();
        assert!(!text.contains(f.live.to_str().unwrap()), "{text}");
        assert!(!text.contains("Call with"), "{text}");
    }
}

/// The top-level keys of the leading `{...}` in `text`, optional ones marked `?`, each with
/// the shape given after its `:` (empty when none).
fn signature(text: &str) -> std::collections::BTreeMap<String, String> {
    let (mut depth, mut entries, mut entry) = (0, vec![], String::new());
    for c in text.chars() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            ',' if depth == 1 => {
                entries.push(std::mem::take(&mut entry));
                continue;
            }
            _ => {}
        }
        if depth == 0 {
            break;
        }
        if depth > 1 || c != '{' {
            entry.push(c);
        }
    }
    entries.push(entry);
    entries
        .iter()
        .map(|e| e.split_once(':').unwrap_or((e, "")))
        .filter(|(key, _)| !key.trim().is_empty())
        .map(|(key, shape)| (key.trim().to_string(), shape.trim().to_string()))
        .collect()
}

/// The help that gives an operation's request shape: edit's, for operations of the same shape.
fn request_shape(help: &'static str) -> &'static str {
    use falinks_claude::engine_host::OPERATIONS;
    if help.starts_with("Same shape as edit") {
        OPERATIONS.iter().find(|(o, ..)| *o == "edit").unwrap().2
    } else {
        help
    }
}

/// Each tool's input schema types every key its help names, so the runtime sends arrays and
/// objects as JSON rather than encoded strings (#239). The help is tied to each parser by
/// `each_description_names_the_keys_its_operation_takes`, and for host tools by
/// `host_tools_refuse_unknown_or_missing_request_keys`.
#[test]
fn each_tool_types_the_keys_its_help_names() {
    use falinks_claude::{arm::HOST_TOOLS, engine_host::OPERATIONS};
    let tools = falinks_claude::arm::tools();
    for (operation, _, help) in OPERATIONS.iter().chain(HOST_TOOLS) {
        let help = request_shape(help);
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == format!("falinks_{operation}"))
            .unwrap();
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{tool}"));
        let keys = signature(help);
        assert_eq!(
            properties
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            keys.keys().map(|k| k.trim_end_matches('?')).collect(),
            "{operation}"
        );
        for (key, shape) in &keys {
            let types = match &properties[key.trim_end_matches('?')]["type"] {
                Value::String(one) => vec![one.as_str()],
                Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
                other => panic!("{operation}.{key} has type {other}"),
            };
            let needs = [
                (shape.starts_with('['), "array"),
                (shape.starts_with('{') || shape.contains("| {"), "object"),
                (shape == "bool", "boolean"),
            ];
            for (_, kind) in needs.iter().filter(|(needed, _)| *needed) {
                assert!(
                    types.contains(kind),
                    "{operation}.{key}: {shape} needs {kind}"
                );
            }
        }
    }
}

/// Each tool description names exactly the keys its operation's parser takes, so the two
/// hand-written lists cannot drift.
#[test]
fn each_description_names_the_keys_its_operation_takes() {
    use falinks_claude::engine_host::OPERATIONS;
    let f = fixture();
    for (operation, _, help) in OPERATIONS {
        let help = request_shape(help);
        let reply = f.call(0, operation, json!({"unknown":true}));
        let reason = reply["reason"].as_str().unwrap_or_default();
        let takes = reason
            .strip_prefix(&format!("request keys: {operation} takes "))
            .unwrap_or_else(|| panic!("{operation}: {reply}"));
        assert_eq!(
            signature(takes).keys().collect::<Vec<_>>(),
            signature(help).keys().collect::<Vec<_>>(),
            "{operation}: {reason}"
        );
    }
}

/// The edit note is required and reaches the peer as the live edit's text (#192).
#[test]
fn the_edit_note_is_required_and_is_the_peers_live_edit_text() {
    let f = fixture();
    let version = f.call(0, "capture", json!({}))["files"]["price.go"]["version"].clone();
    let unnoted = f.call(
        0,
        "edit",
        json!({"id":"d1","expected":{"price.go":version},"output":{"price.go":{"content":DISCOUNT}}}),
    );
    let reason = unnoted["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("missing [\"note\"]"), "{unnoted}");
    let blank = f.call(
        0,
        "edit",
        json!({"id":"d1","note":" ","expected":{"price.go":version},"output":{"price.go":{"content":DISCOUNT}}}),
    );
    // The engine refuses a blank note ("edit note required"); the adapter checks only the key.
    assert_eq!(blank["accepted"], false, "{blank}");
    let note = "Discount takes a tenth of gross";
    let applied = f.call(
        0,
        "edit",
        json!({"id":"d1","note":note,"expected":{"price.go":version},"output":{"price.go":{"content":DISCOUNT}}}),
    );
    assert_eq!(applied["accepted"], true, "{applied}");
    let events = serde_json::to_string(&f.engine.events(&f.clients[1], 0).unwrap()).unwrap();
    assert!(events.contains(note), "{events}");
    assert!(!events.contains("request d1"), "{events}");
}
