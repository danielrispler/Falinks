//! Captured Rust/Go analysis evidence and the conservative related-scope model.
//! Evidence is a pure function of an exact Git tree plus verified toolchain identities.
//! Compiler-derived references are primary; anything missing, stale, broken or
//! unexplained widens to files, units and reverse dependents.
use crate::{Capture, Result, fail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{Receiver, channel},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    Rust,
    Go,
}
/// A host-verified executable. Analysis refuses to run when the bytes differ.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub path: PathBuf,
    pub sha256: String,
}
/// Rust needs `rust-analyzer`, `cargo` and `rustc`; Go needs `gopls` and `go`.
/// `features` are Cargo features or Go build tags.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toolchain {
    pub language: Language,
    pub executables: Vec<Pin>,
    pub features: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub path: PathBuf,
    pub sha256: String,
    pub version: String,
}
/// A crate or package: the widening granularity for uncertain evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub dir: String,
    pub files: BTreeSet<String>,
    pub deps: BTreeSet<String>,
    /// Compiler errors; a broken unit's relationships are not trusted.
    pub broken: Vec<String>,
}
/// Nearest enclosing function/method, otherwise top-level declaration. Byte range.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub id: String,
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub tree: String,
    pub language: Language,
    pub tools: Vec<Tool>,
    pub features: Vec<String>,
    /// Manifests, lockfiles and other inputs whose change invalidates every relationship.
    pub config: BTreeSet<String>,
    pub units: BTreeMap<String, Unit>,
    pub owners: BTreeMap<String, Vec<Owner>>,
    /// (user, used) nodes: `path#owner`, or `path` for uses outside any owner.
    pub edges: BTreeSet<(String, String)>,
    /// Generated/external inputs outside the captured tree: application is blocked.
    pub unknown: Vec<String>,
    /// Analysis failures: every relationship is widened.
    pub errors: Vec<String>,
}

pub fn sha256_file(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}
pub(crate) fn toolchain_key(toolchain: &Toolchain) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(toolchain)?)
    ))
}
fn tool<'a>(toolchain: &'a Toolchain, name: &str) -> Result<&'a Pin> {
    toolchain
        .executables
        .iter()
        .find(|p| p.path.file_name().is_some_and(|n| n == name))
        .ok_or_else(|| format!("toolchain missing pinned {name}").into())
}
/// Verify every pin and return identities. A mismatch is a visible failure, never a fallback.
pub(crate) fn verify(toolchain: &Toolchain) -> Result<Vec<Tool>> {
    let names: &[&str] = match toolchain.language {
        Language::Rust => &["rust-analyzer", "cargo", "rustc"],
        Language::Go => &["gopls", "go"],
    };
    for name in names {
        tool(toolchain, name)?;
    }
    toolchain
        .executables
        .iter()
        .map(|pin| {
            let actual = sha256_file(&pin.path)?;
            if actual != pin.sha256 {
                return fail(format!(
                    "pinned executable changed: {} ({actual})",
                    pin.path.display()
                ));
            }
            let arg = if pin.path.ends_with("go") || pin.path.ends_with("gopls") {
                "version"
            } else {
                "--version"
            };
            let out = Command::new(&pin.path).arg(arg).env_clear().output()?;
            Ok(Tool {
                path: pin.path.clone(),
                sha256: actual,
                version: String::from_utf8_lossy(&out.stdout).trim().into(),
            })
        })
        .collect()
}

/// Analyze an exact capture in a disposable copy. Infrastructure failures become
/// `errors` (conservative widening) rather than missing evidence.
pub(crate) fn analyze(scratch: &Path, capture: &Capture, toolchain: &Toolchain) -> Evidence {
    let mut evidence = Evidence {
        tree: capture.tree.clone(),
        language: toolchain.language,
        tools: Vec::new(),
        features: toolchain.features.clone(),
        config: BTreeSet::new(),
        units: BTreeMap::new(),
        owners: BTreeMap::new(),
        edges: BTreeSet::new(),
        unknown: Vec::new(),
        errors: Vec::new(),
    };
    let result = (|| -> Result<()> {
        evidence.tools = verify(toolchain)?;
        let src = scratch.join("src");
        for (path, file) in &capture.files {
            if let Some(file) = &file.file {
                fs::create_dir_all(src.join(path).parent().ok_or("missing parent")?)?;
                fs::write(src.join(path), &file.bytes)?;
            }
        }
        let env = environment(scratch, toolchain)?;
        match toolchain.language {
            Language::Rust => rust_units(&src, &env, toolchain, capture, &mut evidence)?,
            Language::Go => go_units(&src, &env, toolchain, capture, &mut evidence)?,
        }
        references(&src, &env, toolchain, capture, &mut evidence)?;
        // Analysis must not alter its inputs; otherwise its results describe other bytes.
        let mut observed = Vec::new();
        crate::paths(&src, Path::new(""), &mut observed, false)?;
        for path in &observed {
            let file = capture.files.get(path).and_then(|f| f.file.as_ref());
            if file.is_none_or(|f| fs::read(src.join(path)).ok().as_ref() != Some(&f.bytes)) {
                return fail(format!("analysis changed captured input {path}"));
            }
        }
        if observed.len() != capture.files.values().filter(|f| f.file.is_some()).count() {
            return fail("analysis removed captured input");
        }
        Ok(())
    })();
    if let Err(error) = result {
        evidence.errors.push(error.to_string());
    }
    evidence
}

fn environment(scratch: &Path, toolchain: &Toolchain) -> Result<Vec<(String, String)>> {
    let dirs: BTreeSet<_> = toolchain
        .executables
        .iter()
        .filter_map(|p| p.path.parent().and_then(Path::to_str))
        .collect();
    let path = dirs
        .into_iter()
        .chain(["/usr/bin", "/bin"])
        .collect::<Vec<_>>();
    let s = |p: PathBuf| p.to_str().map(String::from).ok_or("non-UTF8 scratch");
    let mut env = vec![
        ("PATH".into(), path.join(":")),
        ("HOME".into(), s(scratch.join("home"))?),
        ("TMPDIR".into(), s(scratch.join("tmp"))?),
    ];
    fs::create_dir_all(scratch.join("tmp"))?;
    match toolchain.language {
        Language::Rust => {
            env.push(("CARGO".into(), s(tool(toolchain, "cargo")?.path.clone())?));
            env.push(("RUSTC".into(), s(tool(toolchain, "rustc")?.path.clone())?));
            env.push(("CARGO_HOME".into(), s(scratch.join("cargo-home"))?));
            env.push(("CARGO_TARGET_DIR".into(), s(scratch.join("target"))?));
            env.push(("CARGO_NET_OFFLINE".into(), "true".into()));
        }
        Language::Go => {
            let tags = toolchain.features.join(",");
            env.extend([
                ("GOENV".into(), "off".into()),
                ("GOTOOLCHAIN".into(), "local".into()),
                ("GOWORK".into(), "off".into()),
                ("GOPACKAGESDRIVER".into(), "off".into()),
                ("GOPROXY".into(), "off".into()),
                ("GOFLAGS".into(), format!("-mod=mod -tags={tags}")),
                ("CGO_ENABLED".into(), "0".into()),
                ("GOPATH".into(), s(scratch.join("gopath"))?),
                ("GOCACHE".into(), s(scratch.join("gocache"))?),
            ]);
        }
    }
    Ok(env)
}
fn run(
    program: &Path,
    args: &[&str],
    dir: &Path,
    env: &[(String, String)],
) -> Result<(bool, String, String)> {
    let out = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .output()?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into(),
        String::from_utf8_lossy(&out.stderr).into(),
    ))
}
fn relative(src: &Path, path: &str) -> Option<String> {
    let src = fs::canonicalize(src).ok()?;
    let path = fs::canonicalize(path).ok()?;
    path.strip_prefix(src).ok()?.to_str().map(String::from)
}
fn members(capture: &Capture, dir: &str, nested: &[String], suffix: &str) -> BTreeSet<String> {
    capture
        .files
        .iter()
        .filter(|(p, f)| f.file.is_some() && p.ends_with(suffix))
        .map(|(p, _)| p.clone())
        .filter(|p| dir.is_empty() || p.starts_with(&format!("{dir}/")))
        .filter(|p| {
            !nested
                .iter()
                .any(|n| n.len() > dir.len() && p.starts_with(&format!("{n}/")))
        })
        .collect()
}

fn rust_units(
    src: &Path,
    env: &[(String, String)],
    toolchain: &Toolchain,
    capture: &Capture,
    evidence: &mut Evidence,
) -> Result<()> {
    let cargo = &tool(toolchain, "cargo")?.path;
    let features = toolchain.features.join(",");
    let (ok, out, err) = run(
        cargo,
        &[
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--locked",
            "--features",
            &features,
        ],
        src,
        env,
    )?;
    if !ok {
        return fail(format!("cargo metadata failed: {err}"));
    }
    let metadata: Value = serde_json::from_str(&out)?;
    let workspace: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .ok_or("missing workspace members")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let mut dirs = BTreeMap::new();
    for package in metadata["packages"].as_array().ok_or("missing packages")? {
        let id = package["id"].as_str().ok_or("package id")?;
        let name = package["name"].as_str().ok_or("package name")?;
        if !workspace.contains(id) {
            evidence.unknown.push(format!("external crate {name}"));
            continue;
        }
        for target in package["targets"].as_array().ok_or("targets")? {
            for kind in target["kind"].as_array().ok_or("target kind")? {
                if matches!(kind.as_str(), Some("custom-build" | "proc-macro")) {
                    evidence
                        .unknown
                        .push(format!("generated input: {name} {kind}"));
                }
            }
        }
        let manifest = relative(src, package["manifest_path"].as_str().ok_or("manifest")?)
            .ok_or("manifest outside capture")?;
        evidence.config.insert(manifest.clone());
        let dir = manifest.rsplit_once('/').map_or("", |(d, _)| d).to_string();
        dirs.insert(id.to_string(), (format!("rust:{name}"), dir));
    }
    for config in ["Cargo.lock", "rust-toolchain.toml", "rust-toolchain"] {
        if capture.files.contains_key(config) {
            evidence.config.insert(config.into());
        }
    }
    let nested: Vec<_> = dirs.values().map(|(_, d)| d.clone()).collect();
    for node in metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("missing resolve")?
    {
        let Some((unit, dir)) = node["id"].as_str().and_then(|id| dirs.get(id)) else {
            continue;
        };
        let deps = node["deps"]
            .as_array()
            .ok_or("resolve deps")?
            .iter()
            .filter_map(|d| d["pkg"].as_str().and_then(|p| dirs.get(p)))
            .map(|(u, _)| u.clone())
            .collect();
        evidence.units.insert(
            unit.clone(),
            Unit {
                dir: dir.clone(),
                files: members(capture, dir, &nested, ".rs"),
                deps,
                broken: Vec::new(),
            },
        );
    }
    let (ok, out, err) = run(
        cargo,
        &[
            "check",
            "--message-format=json",
            "--offline",
            "--locked",
            "--features",
            &features,
        ],
        src,
        env,
    )?;
    for line in out.lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if message["reason"] == "compiler-message" && message["message"]["level"] == "error" {
            let unit = message["package_id"].as_str().and_then(|p| dirs.get(p));
            let text = message["message"]["message"]
                .as_str()
                .unwrap_or("error")
                .to_string();
            match unit.and_then(|(u, _)| evidence.units.get_mut(u)) {
                Some(unit) => unit.broken.push(text),
                None => evidence.errors.push(text),
            }
        }
    }
    if !ok && evidence.units.values().all(|u| u.broken.is_empty()) {
        evidence.errors.push(format!("cargo check failed: {err}"));
    }
    Ok(())
}

fn go_units(
    src: &Path,
    env: &[(String, String)],
    toolchain: &Toolchain,
    capture: &Capture,
    evidence: &mut Evidence,
) -> Result<()> {
    let go = &tool(toolchain, "go")?.path;
    let (_, out, err) = run(go, &["list", "-e", "-deps", "-json", "./..."], src, env)?;
    let packages: Vec<Value> = serde_json::Deserializer::from_str(&out)
        .into_iter()
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| format!("go list: {e}: {err}"))?;
    if packages.is_empty() {
        return fail(format!("go list returned no packages: {err}"));
    }
    for config in ["go.mod", "go.sum", "go.work"] {
        if capture.files.contains_key(config) {
            evidence.config.insert(config.into());
        }
    }
    let local: BTreeSet<&str> = packages
        .iter()
        .filter(|p| p["Module"]["Main"] == true)
        .filter_map(|p| p["ImportPath"].as_str())
        .collect();
    let mut dirs = Vec::new();
    for package in &packages {
        let path = package["ImportPath"].as_str().ok_or("import path")?;
        if package["Standard"] == true {
            continue;
        }
        if !local.contains(path) {
            evidence.unknown.push(format!("external package {path}"));
            continue;
        }
        if package["CgoFiles"]
            .as_array()
            .is_some_and(|f| !f.is_empty())
        {
            evidence.unknown.push(format!("cgo inputs in {path}"));
        }
        let dir = relative(src, package["Dir"].as_str().ok_or("package dir")?)
            .ok_or("package outside capture")?;
        dirs.push(dir.clone());
        let mut unit = Unit {
            dir,
            files: BTreeSet::new(),
            deps: package["Imports"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|i| local.contains(i))
                .map(|i| format!("go:{i}"))
                .collect(),
            broken: Vec::new(),
        };
        if let Some(error) = package["Error"]["Err"].as_str() {
            unit.broken.push(error.into());
        }
        evidence.units.insert(format!("go:{path}"), unit);
    }
    for unit in evidence.units.values_mut() {
        // Every .go file in the directory, including tag-ignored and test files.
        unit.files = members(capture, &unit.dir.clone(), &[], ".go")
            .into_iter()
            .filter(|p| p.rsplit_once('/').map_or("", |(d, _)| d) == unit.dir)
            .collect();
    }
    let (ok, _, err) = run(go, &["vet", "./..."], src, env)?;
    if !ok {
        let mut current = None;
        let mut attributed = false;
        for line in err.lines() {
            if let Some(path) = line.strip_prefix("# ") {
                current = evidence
                    .units
                    .contains_key(&format!("go:{path}"))
                    .then(|| format!("go:{path}"));
            } else if let Some(unit) = current.as_ref().and_then(|u| evidence.units.get_mut(u)) {
                unit.broken.push(line.into());
                attributed = true;
            }
        }
        if !attributed {
            evidence.errors.push(format!("go vet failed: {err}"));
        }
    }
    Ok(())
}

/// Minimal stdio JSON-RPC client for the two pinned language servers.
struct Lsp {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next: i64,
    settings: Value,
    status: Option<Value>,
}
const LSP_TIMEOUT: Duration = Duration::from_secs(120);
impl Lsp {
    fn start(
        program: &Path,
        dir: &Path,
        env: &[(String, String)],
        log: &Path,
        settings: Value,
    ) -> Result<Self> {
        let mut child = Command::new(program)
            .current_dir(dir)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(fs::File::create(log)?)
            .spawn()?;
        let stdin = child.stdin.take().ok_or("server stdin")?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or("server stdout")?);
        let (tx, rx) = channel();
        std::thread::spawn(move || -> Option<()> {
            loop {
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if stdout.read_line(&mut line).ok()? == 0 {
                        return None;
                    }
                    let line = line.trim();
                    if line.is_empty() {
                        break;
                    }
                    if let Some(n) = line.strip_prefix("Content-Length:") {
                        length = n.trim().parse().ok()?;
                    }
                }
                let mut body = vec![0; length];
                stdout.read_exact(&mut body).ok()?;
                tx.send(serde_json::from_slice(&body).ok()?).ok()?;
            }
        });
        Ok(Self {
            child,
            stdin,
            rx,
            next: 0,
            settings,
            status: None,
        })
    }
    fn send(&mut self, message: Value) -> Result<()> {
        let body = serde_json::to_vec(&message)?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        self.stdin.write_all(&body)?;
        Ok(self.stdin.flush()?)
    }
    fn receive(&mut self, deadline: Instant) -> Result<Value> {
        let message = self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| "language server timed out or exited")?;
        if let (Some(method), Some(id)) = (message["method"].as_str(), message.get("id")) {
            let result = match method {
                "workspace/configuration" => {
                    let n = message["params"]["items"].as_array().map_or(0, Vec::len);
                    json!(vec![self.settings.clone(); n])
                }
                _ => Value::Null,
            };
            self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}))?;
        } else if message["method"] == "experimental/serverStatus" {
            self.status = Some(message["params"].clone());
        }
        Ok(message)
    }
    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let deadline = Instant::now() + LSP_TIMEOUT;
        loop {
            let message = self.receive(deadline)?;
            if message.get("method").is_none() && message["id"] == id {
                if let Some(error) = message.get("error") {
                    return fail(format!("{method}: {error}"));
                }
                return Ok(message["result"].clone());
            }
        }
    }
    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }
}
impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Symbol {
    owner: String,
    selection: (u64, u64),
}
fn offset(bytes: &[u8], position: &Value, utf8: bool) -> Option<usize> {
    let line = position["line"].as_u64()? as usize;
    let character = position["character"].as_u64()? as usize;
    let mut start = 0;
    for _ in 0..line {
        start += bytes[start..].iter().position(|b| *b == b'\n')? + 1;
    }
    let text = std::str::from_utf8(&bytes[start..]).ok()?;
    let mut units = 0;
    for (i, c) in text.char_indices() {
        if units >= character || c == '\n' {
            return Some(start + i);
        }
        units += if utf8 { c.len_utf8() } else { c.len_utf16() };
    }
    Some(bytes.len())
}

fn references(
    src: &Path,
    env: &[(String, String)],
    toolchain: &Toolchain,
    capture: &Capture,
    evidence: &mut Evidence,
) -> Result<()> {
    let (server, settings) = match toolchain.language {
        Language::Rust => (
            "rust-analyzer",
            json!({
                "cargo": {"buildScripts": {"enable": false}, "features": toolchain.features, "extraEnv": {}},
                "procMacro": {"enable": false},
                "checkOnSave": false,
                "cachePriming": {"enable": false},
                "files": {"watcher": "client"},
            }),
        ),
        Language::Go => (
            "gopls",
            json!({"buildFlags": [format!("-tags={}", toolchain.features.join(","))]}),
        ),
    };
    let log = src.parent().ok_or("scratch")?.join(format!("{server}.log"));
    let mut lsp = Lsp::start(
        &tool(toolchain, server)?.path,
        src,
        env,
        &log,
        settings.clone(),
    )?;
    let root = format!(
        "file://{}",
        fs::canonicalize(src)?.to_str().ok_or("non-UTF8")?
    );
    let init = lsp.request(
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": root,
            "workspaceFolders": [{"uri": root, "name": "capture"}],
            "initializationOptions": settings,
            "capabilities": {
                "general": {"positionEncodings": ["utf-8", "utf-16"]},
                "textDocument": {"documentSymbol": {"hierarchicalDocumentSymbolSupport": true}},
                "window": {"workDoneProgress": true},
                "experimental": {"serverStatusNotification": true},
            },
        }),
    )?;
    let utf8 = init["capabilities"]["positionEncoding"] == "utf-8";
    lsp.notify("initialized", json!({}))?;
    if toolchain.language == Language::Rust {
        // Results before a quiescent, healthy load are incomplete; they are not independence.
        let deadline = Instant::now() + LSP_TIMEOUT;
        while !lsp.status.as_ref().is_some_and(|s| s["quiescent"] == true) {
            lsp.receive(deadline)?;
        }
        let status = lsp.status.clone().unwrap_or_default();
        if status["health"] != "ok" {
            evidence
                .errors
                .push(format!("rust-analyzer not healthy: {status}"));
        }
    }
    let suffix = match toolchain.language {
        Language::Rust => ".rs",
        Language::Go => ".go",
    };
    let files: Vec<String> = evidence
        .units
        .values()
        .flat_map(|u| u.files.iter().cloned())
        .collect();
    let uri = |path: &str| format!("{root}/{path}");
    let mut symbols: BTreeMap<String, Vec<Symbol>> = BTreeMap::new();
    for path in files.iter().filter(|p| p.ends_with(suffix)) {
        let bytes = &capture.files[path]
            .file
            .as_ref()
            .ok_or("member missing")?
            .bytes;
        lsp.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri(path), "languageId": if suffix == ".rs" {"rust"} else {"go"}, "version": 1, "text": String::from_utf8_lossy(bytes)}}),
        )?;
        let reply = lsp.request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri(path)}}),
        )?;
        let mut owners = Vec::new();
        let mut found = Vec::new();
        // (symbol, container chain, depth)
        let mut stack: Vec<(Value, String, usize)> = reply
            .as_array()
            .ok_or("flat or missing document symbols")?
            .iter()
            .map(|s| (s.clone(), String::new(), 0))
            .collect();
        while let Some((symbol, prefix, depth)) = stack.pop() {
            let name = format!("{prefix}{}", symbol["name"].as_str().ok_or("symbol name")?);
            let range = &symbol["range"];
            let start = offset(bytes, &range["start"], utf8).ok_or("symbol range")?;
            let end = offset(bytes, &range["end"], utf8).ok_or("symbol range")?;
            // LSP SymbolKind: 6 method, 9 constructor, 12 function.
            if depth == 0 || matches!(symbol["kind"].as_u64(), Some(6 | 9 | 12)) {
                owners.push(Owner {
                    id: format!("{path}#{name}"),
                    start,
                    end,
                });
            }
            let selection = &symbol["selectionRange"]["start"];
            found.push((
                start,
                selection["line"].as_u64().unwrap_or(0),
                selection["character"].as_u64().unwrap_or(0),
            ));
            for child in symbol["children"].as_array().into_iter().flatten() {
                stack.push((child.clone(), format!("{name}::"), depth + 1));
            }
        }
        let owned: Vec<_> = found
            .into_iter()
            .map(|(start, line, character)| Symbol {
                owner: owner_at(&owners, start).map_or_else(|| path.clone(), |o| o.id.clone()),
                selection: (line, character),
            })
            .collect();
        owners.sort_by_key(|o| (o.start, o.end));
        evidence.owners.insert(path.clone(), owners);
        symbols.insert(path.clone(), owned);
    }
    for (path, owned) in &symbols {
        for symbol in owned {
            let params = json!({
                "textDocument": {"uri": uri(path)},
                "position": {"line": symbol.selection.0, "character": symbol.selection.1},
                "context": {"includeDeclaration": false},
            });
            let uses = lsp.request("textDocument/references", params.clone())?;
            // Implementations relate to their trait/interface without any reference
            // edge. Servers refuse the request for symbols with no implementations.
            let implementations = lsp
                .request("textDocument/implementation", params)
                .unwrap_or(Value::Null);
            let implementations = match implementations {
                Value::Object(_) => vec![implementations],
                Value::Array(locations) => locations,
                _ => Vec::new(),
            };
            let locations = uses
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| (l, false))
                .chain(implementations.iter().map(|l| (l, true)));
            for (location, implementation) in locations {
                let Some(user_path) = location["uri"]
                    .as_str()
                    .and_then(|u| u.strip_prefix(&format!("{root}/")))
                else {
                    continue; // toolchain sources are bound by the pinned identity
                };
                let Some(file) = capture.files.get(user_path).and_then(|f| f.file.as_ref()) else {
                    evidence
                        .errors
                        .push(format!("reference in uncaptured file {user_path}"));
                    continue;
                };
                let at = offset(&file.bytes, &location["range"]["start"], utf8)
                    .ok_or("reference range")?;
                let user = evidence
                    .owners
                    .get(user_path)
                    .and_then(|o| owner_at(o, at))
                    .map_or_else(|| user_path.to_string(), |o| o.id.clone());
                if user != symbol.owner {
                    if implementation {
                        evidence.edges.insert((symbol.owner.clone(), user.clone()));
                    }
                    evidence.edges.insert((user, symbol.owner.clone()));
                }
            }
        }
    }
    let _ = lsp.request("shutdown", Value::Null);
    let _ = lsp.notify("exit", Value::Null);
    Ok(())
}
fn owner_at(owners: &[Owner], at: usize) -> Option<&Owner> {
    owners
        .iter()
        .filter(|o| o.start <= at && at < o.end)
        .min_by_key(|o| o.end - o.start)
}

/// Nodes are `path` (the whole file) or `path#owner`. `all` means unbounded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footprint {
    pub all: bool,
    pub nodes: BTreeSet<String>,
}
fn matches(a: &str, b: &str) -> bool {
    // A file covers its owners; a declaration covers its nested members.
    let covers = |outer: &str, inner: &str| {
        inner.strip_prefix(outer).is_some_and(|rest| {
            rest.starts_with('#') && !outer.contains('#') || rest.starts_with("::")
        })
    };
    a == b || covers(a, b) || covers(b, a)
}
pub(crate) fn related(a: &Footprint, b: &Footprint) -> bool {
    a.all
        || b.all
        || a.nodes
            .iter()
            .any(|x| b.nodes.iter().any(|y| matches(x, y)))
}
fn unit_of<'a>(evidence: &'a Evidence, path: &str) -> Option<(&'a String, &'a Unit)> {
    evidence
        .units
        .iter()
        .filter(|(_, u)| {
            u.files.contains(path) || u.dir.is_empty() || path.starts_with(&format!("{}/", u.dir))
        })
        .max_by_key(|(_, u)| (u.files.contains(path), u.dir.len()))
}
/// An uncertain unit widens to its files, its direct dependencies and all reverse dependents.
fn widen(evidence: &Evidence, unit: &str, nodes: &mut BTreeSet<String>) {
    let mut reached: BTreeSet<&str> = evidence.units[unit]
        .deps
        .iter()
        .map(String::as_str)
        .collect();
    let mut frontier = vec![unit];
    while let Some(current) = frontier.pop() {
        if reached.insert(current) {
            frontier.extend(
                evidence
                    .units
                    .iter()
                    .filter(|(_, u)| u.deps.contains(current))
                    .map(|(n, _)| n.as_str()),
            );
        }
    }
    for name in reached {
        if let Some(other) = evidence.units.get(name) {
            nodes.extend(other.files.iter().cloned());
        }
    }
}

/// The owners touched by changing one file, under each side's evidence.
pub(crate) fn change(
    before: &Capture,
    after: &Capture,
    paths: &BTreeSet<String>,
    evidence: &[&[Evidence]],
) -> Footprint {
    let mut footprint = Footprint::default();
    for path in paths {
        let bytes = |c: &Capture| {
            c.files
                .get(path)
                .and_then(|f| f.file.as_ref())
                .map(|f| f.bytes.clone())
                .unwrap_or_default()
        };
        let (old, new) = (bytes(before), bytes(after));
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(new[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        // ponytail: one hunk per file; per-hunk diff only if over-notification hurts.
        for (side, bytes) in evidence.iter().zip([&old, &new]) {
            let region = &bytes[prefix..bytes.len() - suffix];
            touched(side, path, prefix, region, &mut footprint);
        }
    }
    footprint
}
fn touched(side: &[Evidence], path: &str, start: usize, region: &[u8], footprint: &mut Footprint) {
    if side.is_empty() {
        footprint.all = true;
    }
    for evidence in side {
        if !evidence.errors.is_empty()
            || !evidence.unknown.is_empty()
            || evidence.config.contains(path)
        {
            footprint.all = true;
            continue;
        }
        let Some((name, unit)) = unit_of(evidence, path) else {
            footprint.nodes.insert(path.into());
            continue;
        };
        if !unit.broken.is_empty() || !unit.files.contains(path) {
            widen(evidence, name, &mut footprint.nodes);
            continue;
        }
        let owners = evidence
            .owners
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut covered = true;
        for (i, byte) in region.iter().enumerate() {
            match owner_at(owners, start + i) {
                Some(owner) => {
                    footprint.nodes.insert(owner.id.clone());
                }
                None if byte.is_ascii_whitespace() => {}
                None => covered = false,
            }
        }
        if region.is_empty() {
            match owner_at(owners, start) {
                Some(owner) => {
                    footprint.nodes.insert(owner.id.clone());
                }
                None => covered = start == 0 && owners.is_empty(),
            }
        }
        if !covered {
            footprint.nodes.insert(path.into());
        }
    }
}

/// Registered work plus what it uses (transitively) and its direct users.
pub(crate) fn closure(nodes: &BTreeSet<String>, evidence: &[&[Evidence]]) -> Footprint {
    let mut footprint = Footprint {
        all: false,
        nodes: nodes.clone(),
    };
    for side in evidence {
        if side.is_empty() {
            footprint.all = true;
        }
        for e in *side {
            if !e.errors.is_empty() || !e.unknown.is_empty() {
                footprint.all = true;
                continue;
            }
            for node in nodes {
                let path = node.split_once('#').map_or(node.as_str(), |(p, _)| p);
                if let Some((name, unit)) = unit_of(e, path)
                    && (!unit.broken.is_empty() || !unit.files.contains(path))
                {
                    let mut widened = BTreeSet::new();
                    widen(e, name, &mut widened);
                    footprint.nodes.extend(widened);
                }
            }
        }
    }
    let edges: Vec<_> = evidence
        .iter()
        .flat_map(|s| s.iter())
        .flat_map(|e| &e.edges)
        .collect();
    let mut frontier: Vec<String> = footprint.nodes.iter().cloned().collect();
    while let Some(node) = frontier.pop() {
        for (user, used) in &edges {
            if matches(&node, user) && footprint.nodes.insert(used.clone()) {
                frontier.push(used.clone());
            }
        }
    }
    for (user, used) in &edges {
        if nodes.iter().any(|n| matches(n, used)) {
            footprint.nodes.insert(user.clone());
        }
    }
    footprint
}
