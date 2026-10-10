//! Frozen fixture checks and a two-process communicating Git-worktree baseline.
mod baseline;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, fs, thread};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

macro_rules! bail {
    ($($t:tt)*) => { return Err(format!($($t)*).into()) };
}
pub(crate) use bail;

pub const ROOT: &str = env!("CARGO_MANIFEST_DIR");
pub const FIXTURES: [&str; 3] = ["rust-errors", "go-page", "go-relationships"];
pub const FIXED_ENV: [(&str, &str); 10] = [
    ("GIT_AUTHOR_NAME", "Fixture"),
    ("GIT_AUTHOR_EMAIL", "fixture@example.invalid"),
    ("GIT_COMMITTER_NAME", "Fixture"),
    ("GIT_COMMITTER_EMAIL", "fixture@example.invalid"),
    ("GIT_AUTHOR_DATE", "2026-10-07T00:00:00Z"),
    ("GIT_COMMITTER_DATE", "2026-10-07T00:00:00Z"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_NO_REPLACE_OBJECTS", "1"),
    ("GIT_ATTR_NOSYSTEM", "1"),
];
const SCRATCH_DIRS: [&str; 5] = ["tmp", "cache", "home", "bin", "target"];

pub fn root() -> PathBuf {
    PathBuf::from(ROOT)
}

pub fn read(path: impl AsRef<Path>) -> Result<Value> {
    let path = path.as_ref();
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

pub fn write(path: impl AsRef<Path>, data: &Value) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(data)? + "\n")?;
    Ok(())
}

pub fn fixture(name: &str) -> Result<Value> {
    read(root().join("fixtures").join(format!("{name}.json")))
}

pub fn protected(name: &str) -> Result<Value> {
    read(root().join("protected").join(format!("{name}.json")))
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Canonical path when it exists; otherwise absolute.
pub fn resolve(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| std::path::absolute(path).unwrap_or(path.into()))
}

/// Regular files beneath `dir`, sorted.
pub fn walk(dir: &Path) -> Vec<PathBuf> {
    let (mut out, mut stack) = (Vec::new(), vec![dir.to_path_buf()]);
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

pub fn utc(secs: u64) -> String {
    // Civil-from-days (Howard Hinnant); avoids a date dependency for one timestamp format.
    let (days, rem) = ((secs / 86400) as i64, secs % 86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn utc_now() -> String {
    utc(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs())
}

pub struct Output {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned() + &String::from_utf8_lossy(&self.stderr)
    }
}

pub fn kill_group(pid: u32, signal: i32) {
    unsafe { libc::killpg(pid as i32, signal) };
}

/// Runs argv in its own process group; the whole group is killed afterwards.
pub fn command(argv: &[String], cwd: &Path, env: &[(String, String)], timeout: Duration) -> Result<Output> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .env_clear()
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{}: {e}", argv[0]))?;
    let drain = |mut stream: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.read_to_end(&mut bytes);
            bytes
        })
    };
    let stdout = drain(Box::new(child.stdout.take().unwrap()));
    let stderr = drain(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(5));
    };
    kill_group(child.id(), libc::SIGKILL);
    child.wait()?;
    let (stdout, stderr) = (stdout.join().unwrap(), stderr.join().unwrap());
    match status {
        Some(status) => Ok(Output { success: status.success(), stdout, stderr }),
        None => bail!("Command timed out after {timeout:?}: {}", argv.join(" ")),
    }
}

/// macOS sandbox wrapper. Host launches workers/checks; absence of the sandbox is fatal.
#[derive(Clone)]
pub struct Sandbox {
    pub writable: Vec<PathBuf>,
    pub denied: Vec<PathBuf>,
    pub network: bool,
}

impl Sandbox {
    pub fn wrap(&self, argv: Vec<String>) -> Result<Vec<String>> {
        if !cfg!(target_os = "macos") || !Path::new("/usr/bin/sandbox-exec").exists() {
            bail!("This runner requires macOS sandbox-exec; no unprotected fallback");
        }
        let quote = |p: &PathBuf| serde_json::to_string(&resolve(p).to_string_lossy()).unwrap();
        let mut profile = vec![
            "(version 1)".to_string(),
            "(allow default)".into(),
            "(deny file-write*)".into(),
            "(deny network*)".into(),
            "(allow file-write* (literal \"/dev/null\"))".into(),
        ];
        if self.network {
            profile.push("(allow network-outbound)".into());
        }
        profile.extend(self.writable.iter().map(|p| format!("(allow file-write* (subpath {}))", quote(p))));
        profile.extend(self.denied.iter().map(|p| {
            let p = quote(p);
            format!("(deny file-read* (subpath {p})) (deny file-write* (subpath {p}))")
        }));
        let mut wrapped = vec!["/usr/bin/sandbox-exec".to_string(), "-p".into(), profile.join("\n")];
        wrapped.extend(argv);
        Ok(wrapped)
    }
}

pub fn strings(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|s| s.to_string()).collect()
}

pub fn git_env() -> Vec<(String, String)> {
    let mut env = vec![("PATH".to_string(), env::var("PATH").unwrap_or_default())];
    env.extend(FIXED_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    env
}

pub fn git_raw(repo: &Path, args: &[&str], guard: Option<&Sandbox>) -> Result<Vec<u8>> {
    let mut argv = strings(&["git", "-c", "core.hooksPath=/dev/null"]);
    argv.extend(strings(args));
    let argv = match guard {
        Some(guard) => guard.wrap(argv)?,
        None => argv,
    };
    let result = command(&argv, repo, &git_env(), Duration::from_secs(120))?;
    if !result.success {
        bail!("{}", result.text());
    }
    Ok(result.stdout)
}

pub fn git(repo: &Path, args: &[&str], guard: Option<&Sandbox>) -> Result<String> {
    Ok(String::from_utf8_lossy(&git_raw(repo, args, guard)?).trim().to_string())
}

pub fn put_files(dest: &Path, files: &Value) -> Result<()> {
    for (name, text) in files.as_object().ok_or("files must be an object")? {
        let path = dest.join(name);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, text.as_str().ok_or("file text must be a string")?)?;
    }
    Ok(())
}

pub fn initial(dest: &Path, fixture: &Value) -> Result<String> {
    fs::create_dir(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    put_files(dest, &fixture["files"])?;
    git(dest, &["init", "--initial-branch=main"], None)?;
    git(dest, &["add", "."], None)?;
    git(dest, &["commit", "-m", "Frozen initial task"], None)?;
    git(dest, &["rev-parse", "HEAD"], None)
}

fn material_hashes() -> Result<Value> {
    let root = root();
    let mut hashes = Map::new();
    let mut files: Vec<PathBuf> = ["fixtures", "protected", "instructions", "src"]
        .iter()
        .flat_map(|dir| walk(&root.join(dir)))
        .collect();
    files.extend(["config.json", "result-schema.json", "Cargo.toml", "Cargo.lock"].map(|f| root.join(f)));
    for path in files {
        let name = path.strip_prefix(&root)?.to_string_lossy().into_owned();
        hashes.insert(name, sha256(&fs::read(&path)?).into());
    }
    Ok(Value::Object(hashes))
}

fn freeze() -> Result<()> {
    let tmp = tempfile::Builder::new().prefix("falinks-freeze-").tempdir()?;
    let mut commits = Map::new();
    for name in FIXTURES {
        commits.insert(name.into(), initial(&tmp.path().join(name), &fixture(name)?)?.into());
    }
    write(root().join("manifest.json"), &json!({"version": 1, "hashes": material_hashes()?, "initial_commits": commits}))
}

pub fn frozen() -> Result<Value> {
    let manifest = read(root().join("manifest.json"))?;
    if material_hashes()? != manifest["hashes"] {
        bail!("Frozen materials changed: version repairs and explicitly run freeze before a new batch");
    }
    Ok(manifest)
}

/// This checkout, its shared Git storage (which holds oracle bytes) and every worktree of it.
pub fn protected_roots() -> Result<Vec<PathBuf>> {
    let root = root();
    let common = PathBuf::from(git(&root, &["rev-parse", "--git-common-dir"], None)?);
    let common = if common.is_absolute() { common } else { root.join(common) };
    let mut roots = vec![resolve(root.parent().unwrap()), resolve(&common)];
    for line in git(&root, &["worktree", "list", "--porcelain"], None)?.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            roots.push(resolve(Path::new(path)));
        }
    }
    Ok(roots)
}

fn run_checks(commands: &[Vec<String>], work: &Path, env: &[(String, String)], sandbox: &Sandbox,
              deadline: Instant, stop_on_failure: bool) -> Result<Value> {
    let mut results = Vec::new();
    for argv in commands {
        let remaining = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(10));
        let result = command(&sandbox.wrap(argv.clone())?, work, env, remaining)?;
        results.push(json!({"command": argv, "passed": result.success, "output": result.text()}));
        if stop_on_failure && !result.success {
            break;
        }
    }
    Ok(Value::Array(results))
}

fn all_passed(checks: &Value) -> bool {
    checks.as_array().unwrap().iter().all(|c| c["passed"] == true)
}

fn restore(work: &Path, source: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    if work.exists() {
        fs::remove_dir_all(work)?;
    }
    fs::create_dir(work)?;
    for (name, data) in source {
        let target = work.join(name);
        fs::create_dir_all(target.parent().unwrap())?;
        fs::write(target, data)?;
    }
    for name in SCRATCH_DIRS {
        fs::create_dir(work.join(name))?;
    }
    Ok(())
}

/// Checks execute only in a host-owned copy, with protected tests and fixed build inputs.
pub fn check(candidate: &Path, fixture: &Value, private: &Path, timeout: Duration, denied_roots: &[PathBuf]) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    let work = private.join("check");
    // Raw tree blobs preserve exact committed bytes; archive export attributes do not.
    let tree = git_raw(candidate, &["ls-tree", "-rz", "--full-tree", "HEAD"], None)?;
    let mut source = BTreeMap::new();
    for entry in tree.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let tab = entry.iter().position(|b| *b == b'\t').ok_or("Malformed tree entry")?;
        let metadata = String::from_utf8_lossy(&entry[..tab]).into_owned();
        let name = String::from_utf8(entry[tab + 1..].to_vec())?;
        let fields: Vec<&str> = metadata.split_whitespace().collect();
        let path = Path::new(&name);
        if !matches!(fields[..], ["100644" | "100755", "blob", _])
            || path.is_absolute()
            || path.components().any(|c| c.as_os_str() == "..")
        {
            bail!("Candidate contains a non-regular or unsafe path");
        }
        let data = git_raw(candidate, &["cat-file", "blob", fields[2]], None)?;
        if data.len() > 1_000_000 {
            bail!("Fixture file exceeds 1 MB bound");
        }
        source.insert(name, data);
    }
    for name in ["Cargo.toml", "Cargo.lock", "go.mod"] {
        if let Some(frozen) = fixture["files"][name].as_str()
            && source.get(name).map(Vec::as_slice) != Some(frozen.as_bytes())
        {
            bail!("Fixture dependencies/build configuration must remain frozen");
        }
    }
    if source.keys().any(|name| name == "build.rs" || name.starts_with(".cargo/")) {
        bail!("Custom build configuration is outside the fixture contract");
    }
    restore(&work, &source)?;

    let mut env: Vec<(String, String)> = env::vars()
        .filter(|(key, _)| ["PATH", "RUSTUP_HOME", "CARGO_HOME", "DEVELOPER_DIR", "SDKROOT"].contains(&key.as_str()))
        .collect();
    // rustup proxies locate toolchains through HOME unless RUSTUP_HOME is explicit.
    if let (Err(_), Ok(home)) = (env::var("RUSTUP_HOME"), env::var("HOME")) {
        env.push(("RUSTUP_HOME".into(), format!("{home}/.rustup")));
    }
    let at = |name: &str| work.join(name).to_string_lossy().into_owned();
    env.extend([
        ("HOME".to_string(), at("home")),
        ("TMPDIR".into(), at("tmp")),
        ("GOCACHE".into(), at("cache")),
        ("GOPATH".into(), at("home")),
        ("GOPROXY".into(), "off".into()),
        ("GOSUMDB".into(), "off".into()),
        ("GOTOOLCHAIN".into(), "local".into()),
        ("CARGO_TARGET_DIR".into(), at("target")),
    ]);
    let parent = private.parent().unwrap();
    let mut denied = protected_roots()?;
    denied.push(parent.join("agents"));
    denied.extend(denied_roots.iter().cloned());
    // Capture is the only allowed exception beneath private state: deny sibling paths individually.
    denied.extend(fs::read_dir(parent)?.flatten().map(|e| e.path()).filter(|p| p != private));
    denied.extend(fs::read_dir(private)?.flatten().map(|e| e.path()).filter(|p| *p != work));
    let sandbox = Sandbox { writable: SCRATCH_DIRS.map(|n| work.join(n)).to_vec(), denied, network: false };

    let rust = fixture["language"] == "rust";
    let commands: Vec<Vec<String>> = if rust {
        vec![strings(&["cargo", "check", "--offline", "--locked"]), strings(&["cargo", "test", "--offline", "--locked"])]
    } else {
        vec![strings(&["go", "test", "./..."])]
    };
    let mut visible = run_checks(&commands, &work, &env, &sandbox, deadline, false)?;
    let source_changed = source.iter().any(|(name, data)| {
        let path = work.join(name);
        !path.is_file() || path.is_symlink() || fs::read(&path).ok().as_ref() != Some(data)
    });
    if source_changed {
        visible.as_array_mut().unwrap().push(
            json!({"command": ["source-integrity"], "passed": false, "output": "Check modified captured source"}));
    }

    // Independent oracle always starts from original captured bytes, even after a mutating visible check.
    restore(&work, &source)?;
    // Agent tests are useful feedback but cannot alter or replace the independent oracle.
    for path in walk(&work) {
        let name = path.strip_prefix(&work)?.to_string_lossy().into_owned();
        if name.ends_with("_test.go") || (name.starts_with("tests/") && name.ends_with(".rs")) {
            fs::remove_file(path)?;
        }
    }
    let oracle = protected(fixture["name"].as_str().unwrap())?["oracle"].as_str().unwrap().to_string();
    let oracle_commands = if rust {
        fs::write(work.join("oracle.rs"), oracle)?;
        vec![
            strings(&["rustc", "--edition=2021", "--crate-name", "catalog", "--crate-type", "lib", "src/lib.rs", "-o", "bin/libcatalog.rlib"]),
            strings(&["rustc", "--edition=2021", "--test", "oracle.rs", "--extern", "catalog=bin/libcatalog.rlib", "-o", "bin/oracle"]),
            vec![at("bin/oracle")],
        ]
    } else {
        fs::write(work.join("oracle_test.go"), oracle)?;
        vec![strings(&["go", "test", "-count=1", "./..."])]
    };
    let results = run_checks(&oracle_commands, &work, &env, &sandbox, deadline, true)?;
    Ok(json!({
        "visible": {"passed": all_passed(&visible), "checks": visible},
        "oracle": {"passed": all_passed(&results), "checks": results},
        "candidate": git(candidate, &["rev-parse", "HEAD"], None)?,
    }))
}

fn verify() -> Result<i32> {
    let manifest = frozen()?;
    let tmp = tempfile::Builder::new().prefix("falinks-verify-").tempdir()?;
    let base = resolve(tmp.path());
    let mut results = Map::new();
    let timeout = Duration::from_secs(120);
    for name in FIXTURES {
        let fixture = fixture(name)?;
        let repo = base.join(name);
        let sha = initial(&repo, &fixture)?;
        if manifest["initial_commits"][name] != sha.as_str() {
            bail!("Initial commit is not reproducible");
        }
        let private = base.join("private");
        fs::create_dir_all(&private)?;
        let first = check(&repo, &fixture, &private, timeout, &[])?;
        put_files(&repo, &protected(name)?["reference"])?;
        git(&repo, &["add", "."], None)?;
        git(&repo, &["commit", "-m", "Known-correct reference"], None)?;
        let reference = check(&repo, &fixture, &private, timeout, &[])?;
        results.insert(name.into(), json!({
            "initial": first["oracle"], "reference": reference["oracle"],
            "initial_visible": first["visible"], "reference_visible": reference["visible"],
            "initial_commit": sha, "reference_commit": reference["candidate"],
        }));
    }
    let ok = results.values().all(|r| {
        r["initial"]["passed"] == false && r["reference"]["passed"] == true
            && r["initial_visible"]["passed"] == true && r["reference_visible"]["passed"] == true
    });
    println!("{}", serde_json::to_string_pretty(&json!({"fixtures": results}))?);
    Ok(if ok { 0 } else { 1 })
}

fn schedule() -> Value {
    let mut pairs = Vec::new();
    for (index, name) in FIXTURES.iter().enumerate() {
        for repetition in 0..3 {
            let arms = if (index + repetition) % 2 == 0 { ["git", "falinks"] } else { ["falinks", "git"] };
            pairs.push(json!({"pair": format!("{name}-{}", repetition + 1), "fixture": name, "arms": arms}));
        }
    }
    json!({"pilot": {"fixture": "go-relationships", "arms": ["git", "falinks"], "scored": false}, "pairs": pairs})
}

const USAGE: &str = "usage: falinks-eval <command>
  freeze
  verify
  schedule
  prepare <fixture> --output DIR
  oracle <fixture> --repo DIR --evidence DIR [--deny-root DIR]...
  baseline <fixture> --config FILE --output DIR [--pair NAME] [--order N]";

struct Args {
    positional: Vec<String>,
    options: Vec<(String, String)>,
}

impl Args {
    fn parse(raw: Vec<String>) -> Result<Self> {
        let (mut positional, mut options, mut raw) = (Vec::new(), Vec::new(), raw.into_iter());
        while let Some(arg) = raw.next() {
            match arg.strip_prefix("--") {
                Some(key) => options.push((key.to_string(), raw.next().ok_or(format!("--{key} needs a value"))?)),
                None => positional.push(arg),
            }
        }
        Ok(Args { positional, options })
    }
    fn all(&self, key: &str) -> Vec<&str> {
        self.options.iter().filter(|(k, _)| k == key).map(|(_, v)| v.as_str()).collect()
    }
    fn get(&self, key: &str) -> Option<&str> {
        self.all(key).last().copied()
    }
    fn required(&self, key: &str) -> Result<&str> {
        Ok(self.get(key).ok_or(format!("--{key} is required\n{USAGE}"))?)
    }
    fn fixture(&self) -> Result<&str> {
        match self.positional.get(1) {
            Some(name) if FIXTURES.contains(&name.as_str()) => Ok(name),
            _ => bail!("fixture must be one of {FIXTURES:?}\n{USAGE}"),
        }
    }
}

fn run() -> Result<i32> {
    let args = Args::parse(env::args().skip(1).collect())?;
    match args.positional.first().map(String::as_str) {
        Some("freeze") => freeze().map(|_| 0),
        Some("verify") => verify(),
        Some("schedule") => {
            println!("{}", serde_json::to_string_pretty(&schedule())?);
            Ok(0)
        }
        Some("prepare") => {
            let manifest = frozen()?;
            let name = args.fixture()?;
            let sha = initial(&resolve(Path::new(args.required("output")?)), &fixture(name)?)?;
            if manifest["initial_commits"][name] != sha.as_str() {
                bail!("Initial commit differs from manifest");
            }
            println!("{}", json!({"initial_snapshot": sha}));
            Ok(0)
        }
        Some("oracle") => {
            frozen()?;
            let name = args.fixture()?;
            let evidence = resolve(Path::new(args.required("evidence")?));
            fs::create_dir(&evidence)?;
            let evidence = resolve(&evidence);
            let denied: Vec<PathBuf> = args.all("deny-root").iter().map(|p| resolve(Path::new(p))).collect();
            let result = check(&resolve(Path::new(args.required("repo")?)), &fixture(name)?, &evidence,
                               Duration::from_secs(120), &denied)?;
            write(evidence.join("checks.json"), &result)?;
            let passed = result["oracle"]["passed"] == true && result["visible"]["passed"] == true;
            println!("{}", json!({"candidate": result["candidate"], "passed": passed}));
            Ok(if passed { 0 } else { 1 })
        }
        Some("baseline") => baseline::baseline(
            args.fixture()?,
            Path::new(args.required("config")?),
            Path::new(args.required("output")?),
            args.get("pair").unwrap_or("unscored"),
            args.get("order").unwrap_or("0").parse()?,
        ),
        _ => bail!("{USAGE}"),
    }
}

fn main() {
    std::process::exit(run().unwrap_or_else(|error| {
        eprintln!("{error}");
        1
    }));
}

#[cfg(test)]
mod tests {
    #[test]
    fn utc_formats_known_instants() {
        assert_eq!(super::utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::utc(1_791_590_400), "2026-10-10T00:00:00Z");
        assert_eq!(super::utc(951_782_400 + 3661), "2000-02-29T01:01:01Z");
    }
}
