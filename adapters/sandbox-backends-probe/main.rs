//! Throwaway probe for issue #34. One binary plays three roles:
//! - `host`: builds a fixture per run, launches the child under each candidate
//!   mechanism, enforces the timeout and reports `PROBE_RESULT` JSON;
//! - `wrap` (Linux): installs a mechanism in its own process, then execs the child;
//! - `child`: attempts every sandbox contract violation and records the outcome.
use serde_json::{Value, json};
use std::{
    env, fs,
    io::Read,
    net::TcpListener,
    path::{Path, PathBuf},
    process,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

mod child;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Contract timeout is 30 s; the probe uses a short one, the kill path is identical.
const HANG_TIMEOUT: Duration = Duration::from_secs(3);
const RUN_TIMEOUT: Duration = Duration::from_secs(30);
/// Set in the host so the child can show whether the environment was cleared.
const CANARY: &str = "FALINKS_PROBE_CANARY";

/// Per-run layout: the child may read `input`, write `output`, and nothing else.
pub struct Fixture {
    pub root: PathBuf,
    pub input: PathBuf,
    pub output: PathBuf,
    pub secret: PathBuf,
    pub evidence: PathBuf,
}

impl Fixture {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let root = base.join(name);
        let fixture = Fixture {
            input: root.join("input"),
            output: root.join("output"),
            secret: root.join("secret"),
            evidence: root.join("evidence"),
            root,
        };
        for dir in [
            &fixture.input,
            &fixture.output,
            &fixture.secret,
            &fixture.evidence,
        ] {
            fs::create_dir_all(dir)?;
        }
        fs::write(fixture.input.join("in.txt"), b"input\n")?;
        fs::write(fixture.secret.join("secret.txt"), b"secret\n")?;
        Ok(fixture)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Scenario {
    /// Child runs its checks, leaves a detached descendant behind and exits.
    Normal,
    /// Child runs its checks, leaves a descendant, then hangs past the timeout.
    Hang,
    /// The mechanism is made unavailable; the child must never run.
    Unavailable,
}

impl Scenario {
    fn name(self) -> &'static str {
        match self {
            Scenario::Normal => "normal",
            Scenario::Hang => "hang",
            Scenario::Unavailable => "unavailable",
        }
    }
}

/// Loopback listener in the host; reports whether the child reached it.
pub struct Loopback {
    pub port: u16,
    hits: mpsc::Receiver<()>,
}

impl Loopback {
    fn new() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let (send, hits) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                drop(stream);
                let _ = send.send(());
            }
        });
        Ok(Loopback { port, hits })
    }
    fn reached(&self) -> bool {
        self.hits.try_iter().count() > 0
    }
}

/// Child argv after the program path.
pub fn child_args(fixture: &Fixture, loopback: &Loopback, scenario: Scenario) -> Vec<String> {
    let mut args = vec![
        "child".to_string(),
        fixture.root.display().to_string(),
        loopback.port.to_string(),
        user_home().display().to_string(),
    ];
    if scenario == Scenario::Hang {
        args.push("hang".into());
    }
    args
}

/// The real user's home, outside the fixture; the child must not list it.
pub fn user_home() -> PathBuf {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into())
}

/// Exit observation produced by a platform launcher.
pub struct Launch {
    pub started: bool,
    pub launch_error: Option<String>,
    pub exit: Option<String>,
    pub timed_out: bool,
    /// Whether anything of the assigned process tree was still alive after the
    /// launcher's own completion check (process group / job accounting).
    pub tree_survivors: Option<bool>,
    pub extra: Value,
}

/// Heartbeat written by the detached descendant; changes only while it lives.
/// Works across PID namespaces, where the recorded pid is meaningless to the host.
fn descendant_alive(output: &Path) -> Option<bool> {
    let read = || fs::read(output.join("heartbeat")).ok();
    let before = read()?;
    thread::sleep(Duration::from_millis(800));
    Some(read() != Some(before))
}

fn record(
    platform: &str,
    mechanism: &str,
    scenario: Scenario,
    fixture: &Fixture,
    loopback: &Loopback,
    launch: Launch,
) -> Value {
    let child = fs::read_to_string(fixture.output.join("child.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let mut stderr = String::new();
    if let Ok(mut file) = fs::File::open(fixture.evidence.join("stderr")) {
        let _ = file.read_to_string(&mut stderr);
    }
    stderr.truncate(2000);
    let alive = descendant_alive(&fixture.output);
    json!({
        "platform": platform,
        "mechanism": mechanism,
        "scenario": scenario.name(),
        "started": launch.started,
        "launch_error": launch.launch_error,
        "exit": launch.exit,
        "timed_out": launch.timed_out,
        "tree_survivors_after_exit": launch.tree_survivors,
        "child_ran": child.is_some(),
        "child": child,
        "loopback_reached_host": loopback.reached(),
        "detached_descendant_alive_after_cleanup": alive,
        "stderr": stderr,
        "extra": launch.extra,
    })
}

/// Waits for `poll` to report an exit or the deadline; returns (exit, timed_out).
pub fn wait_until(
    timeout: Duration,
    mut poll: impl FnMut() -> Result<Option<String>>,
) -> Result<(Option<String>, bool)> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(exit) = poll()? {
            return Ok((Some(exit), false));
        }
        if Instant::now() >= deadline {
            return Ok((None, true));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn host() -> Result<()> {
    // SAFETY: single-threaded at this point; no other thread reads the environment.
    unsafe { env::set_var(CANARY, "leaked") };
    let base = env::temp_dir().join(format!("falinks-sandbox-probe-{}", process::id()));
    let mut results = Vec::new();
    #[cfg(target_os = "linux")]
    let (platform, facts, plan) = ("linux", linux::facts(), linux::plan());
    #[cfg(windows)]
    let (platform, facts, plan) = ("windows", windows::facts(), windows::plan());
    #[cfg(not(any(target_os = "linux", windows)))]
    let (platform, facts, plan): (&str, Value, Vec<(&str, Scenario)>) =
        ("unsupported", json!({}), Vec::new());
    for (mechanism, scenario) in plan {
        let fixture = Fixture::new(&base, &format!("{mechanism}-{}", scenario.name()))?;
        let loopback = Loopback::new()?;
        let timeout = if scenario == Scenario::Hang {
            HANG_TIMEOUT
        } else {
            RUN_TIMEOUT
        };
        #[cfg(target_os = "linux")]
        let launch = linux::launch(mechanism, scenario, &fixture, &loopback, timeout);
        #[cfg(windows)]
        let launch = windows::launch(mechanism, scenario, &fixture, &loopback, timeout);
        #[cfg(not(any(target_os = "linux", windows)))]
        let launch: Result<Launch> = {
            let _ = timeout;
            Err("unsupported platform".into())
        };
        let launch = launch.unwrap_or_else(|error| Launch {
            started: false,
            launch_error: Some(error.to_string()),
            exit: None,
            timed_out: false,
            tree_survivors: None,
            extra: Value::Null,
        });
        let result = record(platform, mechanism, scenario, &fixture, &loopback, launch);
        println!("PROBE_RESULT={result}");
        results.push(result);
        #[cfg(target_os = "linux")]
        linux::cleanup(&fixture);
        #[cfg(windows)]
        windows::cleanup(&fixture);
    }
    let report = json!({ "platform": platform, "facts": facts, "results": results });
    if let Some(path) = env::args().nth(2) {
        fs::write(path, serde_json::to_string_pretty(&report)?)?;
    }
    let _ = fs::remove_dir_all(&base);
    Ok(())
}

fn run() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("host") => host(),
        Some("child") => child::run(&args[2..]),
        Some("descendant") => child::descendant(&args[2..]),
        #[cfg(target_os = "linux")]
        Some("wrap") => linux::wrap(&args[2..]),
        _ => Err("usage: sandbox-backends-probe host [REPORT.json]".into()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}
