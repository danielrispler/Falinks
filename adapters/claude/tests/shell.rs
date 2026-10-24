//! Per-command confinement (#236 spike): `falinks-claude-shell` run as Claude Code 2.1.308
//! runs its prefix, `bash -c "'PREFIX' 'COMMAND'"`, with the environment `settings` gives
//! the runtime, on the #33 fixture layout.
use falinks_claude::{Launch, settings};
use std::{
    fs,
    io::ErrorKind,
    net::TcpListener,
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Command, Output},
};

const SHELL: &str = env!("CARGO_BIN_EXE_falinks-claude-shell");

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    launch: Launch,
}

/// Short `/tmp` paths: a Unix socket path must fit in 104 bytes on macOS.
fn fixture() -> Fixture {
    let dir = tempfile::Builder::new()
        .prefix("f236-")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path().canonicalize().unwrap();
    for name in [
        "source",
        "scratch",
        "controller",
        "snapshots",
        "validation",
        "worker-tools",
        "open",
    ] {
        fs::create_dir(root.join(name)).unwrap();
    }
    fs::write(root.join("source/source.txt"), "original\n").unwrap();
    fs::write(root.join("controller/token"), "TOKEN-236\n").unwrap();
    fs::write(root.join("validation/secret.txt"), "protected\n").unwrap();
    fs::write(root.join("snapshots/secret.txt"), "protected\n").unwrap();
    // Stands in for the relay: reads the token, as `falinks-claude-tool mcp` does.
    let relay = root.join("worker-tools/falinks-claude-tool");
    fs::write(&relay, "#!/bin/sh\ncat \"$2/token\"\n").unwrap();
    fs::set_permissions(&relay, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir(root.join("scratch/.tmp")).unwrap();
    let launch = Launch::fixture(&root);
    Fixture {
        _dir: dir,
        root,
        launch,
    }
}

impl Fixture {
    fn env(&self) -> Vec<(String, String)> {
        let settings = settings(
            &self.launch,
            &self.launch.worker_tools.join("falinks-claude-tool"),
        )
        .unwrap();
        settings["env"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
            .collect()
    }
    /// `command` through `prefix` as Claude Code composes it.
    fn through(&self, prefix: &str, command: &str) -> Output {
        Command::new("/bin/bash")
            .arg("-c")
            .arg(shlex::try_join([prefix, command]).unwrap())
            .envs(self.env())
            .env("GOCACHE", self.root.join("scratch/go-cache"))
            .current_dir(&self.launch.scratch)
            .output()
            .unwrap()
    }
    fn run(&self, command: &str) -> Output {
        self.through(SHELL, command)
    }
    fn path(&self, name: &str) -> String {
        self.root.join(name).display().to_string()
    }
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
}

#[test]
fn writes_land_only_beneath_scratch() {
    let f = fixture();
    let ok = f.run(&format!("echo ok > {}", f.path("scratch/a.txt")));
    assert!(ok.status.success(), "{}", text(&ok));
    assert_eq!(
        fs::read_to_string(f.root.join("scratch/a.txt")).unwrap(),
        "ok\n"
    );
    for target in [
        "source/source.txt",
        "source/new.txt",
        "worker-tools/new",
        "controller/token",
        "snapshots/secret.txt",
        "open/new.txt",
    ] {
        let out = f.run(&format!("echo BYPASS > {}", f.path(target)));
        assert!(!out.status.success(), "{target}: {}", text(&out));
    }
    let out = f.run(&format!("rm {}", f.path("source/source.txt")));
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(
        fs::read_to_string(f.root.join("source/source.txt")).unwrap(),
        "original\n"
    );
    assert_eq!(
        fs::read_to_string(f.root.join("controller/token")).unwrap(),
        "TOKEN-236\n"
    );
    assert!(!f.root.join("open/new.txt").exists());
    // A background descendant inherits the confinement.
    f.run(&format!(
        "(sleep 0.2; echo BYPASS > {}) & wait",
        f.path("source/bg.txt")
    ));
    assert!(!f.root.join("source/bg.txt").exists());
}

#[test]
fn reads_stop_at_the_controller_and_protected_storage() {
    let f = fixture();
    let out = f.run(&format!("cat {}", f.path("source/source.txt")));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "original\n");
    for secret in [
        "controller/token",
        "validation/secret.txt",
        "snapshots/secret.txt",
    ] {
        let out = f.run(&format!("cat {}", f.path(secret)));
        assert!(!out.status.success(), "{secret}");
        assert!(!text(&out).contains("TOKEN-236") && !text(&out).contains("protected"));
    }
}

#[test]
fn no_socket_connects_not_even_to_the_host_channel() {
    let f = fixture();
    let host = UnixListener::bind(f.root.join("controller/host.sock")).unwrap();
    let open = UnixListener::bind(f.root.join("open/s.sock")).unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = tcp.local_addr().unwrap().port();
    for listener in [&host, &open] {
        listener.set_nonblocking(true).unwrap();
    }
    tcp.set_nonblocking(true).unwrap();
    for socket in ["controller/host.sock", "open/s.sock"] {
        let out = f.run(&format!(
            "curl -sS --max-time 3 --unix-socket {} http://falinks/",
            f.path(socket)
        ));
        assert!(!out.status.success(), "{socket}: {}", text(&out));
    }
    let out = f.run(&format!("echo > /dev/tcp/127.0.0.1/{port}"));
    assert!(!out.status.success(), "{}", text(&out));
    let refused = |e: std::io::Error| e.kind() == ErrorKind::WouldBlock;
    assert!(host.accept().err().is_some_and(refused));
    assert!(open.accept().err().is_some_and(refused));
    assert!(tcp.accept().err().is_some_and(refused));
    // The check can see a connection: the same command outside the wrapper connects.
    let mut control = Command::new("curl")
        .args(["-sS", "--max-time", "3", "--unix-socket"])
        .arg(f.root.join("open/s.sock"))
        .arg("http://falinks/")
        .spawn()
        .unwrap();
    open.set_nonblocking(false).unwrap();
    drop(open.accept().unwrap());
    control.wait().unwrap();
}

#[test]
fn only_the_exact_relay_line_runs_outside_and_a_confined_caller_stays_confined() {
    let f = fixture();
    let relay = shlex::try_join([
        &f.path("worker-tools/falinks-claude-tool"),
        "mcp",
        &f.path("controller"),
    ])
    .unwrap();
    // As Claude Code launches the stdio MCP server: the prefix with one argument, no shell.
    let out = Command::new(SHELL)
        .arg(&relay)
        .envs(f.env())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "TOKEN-236\n");
    let nested = f.run(&shlex::try_join([SHELL, &relay]).unwrap());
    let extra = f.run(&format!("{relay} extra"));
    for out in [nested, extra] {
        assert!(!text(&out).contains("TOKEN-236"), "{}", text(&out));
    }
}

#[test]
fn a_wrapper_that_cannot_confine_runs_nothing() {
    let f = fixture();
    let marker = f.root.join("scratch/ran");
    let command = format!("touch {}", marker.display());
    let missing = f.through(&f.path("worker-tools/no-such-shell"), &command);
    assert_eq!(missing.status.code(), Some(127), "{}", text(&missing));
    let valid = f
        .env()
        .into_iter()
        .find(|(k, _)| k == "FALINKS_SHELL")
        .unwrap()
        .1;
    let broken = |key: &str, value: serde_json::Value| {
        let mut spec: serde_json::Value = serde_json::from_str(&valid).unwrap();
        spec[key] = value;
        spec.to_string()
    };
    // No spec, a missing scratch, and a denied root that is not a string, which must not
    // be dropped.
    for spec in [
        String::new(),
        broken("scratch", serde_json::json!(f.root.join("gone"))),
        broken("denied", serde_json::json!([f.root.join("controller"), 7])),
    ] {
        let out = Command::new("/bin/bash")
            .arg("-c")
            .arg(shlex::try_join([SHELL, &command]).unwrap())
            .envs(f.env())
            .env("FALINKS_SHELL", spec)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(126), "{}", text(&out));
    }
    assert!(!marker.exists());
}

/// What the worker's Bash runs today needs neither the network nor writes outside scratch.
#[test]
fn go_builds_and_vets_in_scratch() {
    let f = fixture();
    let module = f.root.join("scratch/module");
    fs::create_dir(&module).unwrap();
    fs::write(module.join("go.mod"), "module m\n\ngo 1.21\n").unwrap();
    fs::write(
        module.join("m.go"),
        "package m\n\nfunc One() int { return 1 }\n",
    )
    .unwrap();
    let out = f.run(&format!(
        "cd {} && GOTOOLCHAIN=local GOPROXY=off go vet ./... && go build ./...",
        module.display()
    ));
    assert!(out.status.success(), "{}", text(&out));
}

#[test]
fn settings_turn_claude_codes_sandbox_off_and_wrap_bash_only() {
    let f = fixture();
    let tool = f.launch.worker_tools.join("falinks-claude-tool");
    let settings = settings(&f.launch, &tool).unwrap();
    assert_eq!(settings["sandbox"], serde_json::json!({"enabled": false}));
    let hook = &settings["hooks"]["PreToolUse"][0]["hooks"][0];
    // Exec form: Claude Code does not put the prefix in front of it.
    assert_eq!(hook["command"], serde_json::json!(tool));
    assert_eq!(
        hook["args"],
        serde_json::json!(["hook", f.launch.controller])
    );
    let spec: serde_json::Value =
        serde_json::from_str(settings["env"]["FALINKS_SHELL"].as_str().unwrap()).unwrap();
    // The MCP server entry mcp.json gets.
    assert_eq!(
        spec["relay"],
        serde_json::json!({"command": tool, "args": ["mcp", f.launch.controller]})
    );
    assert_eq!(spec["shell"], settings["env"]["CLAUDE_CODE_SHELL"]);
    assert!(Path::new(settings["env"]["TMPDIR"].as_str().unwrap()).starts_with(&f.launch.scratch));
}
