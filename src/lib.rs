//! Host-owned, single-repository protocol. Clients have opaque authenticated handles.
//! No publication API is enabled. See docs/engine.md for the supported boundary.
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, RwLock},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(message.into().into())
}
fn nonce() -> Result<String> {
    let mut bytes = [0; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub bytes: Vec<u8>,
    pub executable: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedFile {
    pub version: u64,
    pub file: Option<File>,
    pub blob: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capture {
    pub workspace: String,
    pub revision: u64,
    pub tree: String,
    pub files: BTreeMap<String, VersionedFile>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    /// All file/package inputs, including every output path. Versions are occurrences.
    pub expected: BTreeMap<String, u64>,
    /// Complete file replacements; None deletes an enrolled file.
    pub output: BTreeMap<String, Option<File>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub expected: u64,
    pub current: VersionedFile,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Applied {
        revision: u64,
        tree: String,
    },
    Stale {
        conflicts: BTreeMap<String, Conflict>,
    },
    Interrupted {
        reason: String,
    },
    Rejected {
        reason: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub before: Capture,
    pub proposed: Option<Capture>,
    pub outcome: Option<Outcome>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub operation: u64,
    pub attempts: Vec<Attempt>,
    pub installation_pending: bool,
    pub agent: usize,
    pub session: String,
    pub request: Request,
    pub before: Capture,
    pub proposed: Option<Capture>,
    pub outcome: Option<Outcome>,
}
#[derive(Clone)]
pub struct Client {
    agent: usize,
    credential: String,
    workspace: String,
}
#[derive(Serialize, Deserialize)]
struct Identity {
    workspace: String,
    credentials: [String; 2],
}

pub struct Engine {
    live: PathBuf,
    state: PathBuf,
    identity: Identity,
    // ponytail: one writer mutex; split only if short-operation throughput warrants it.
    writer: Mutex<Connection>,
    completed: RwLock<Capture>,
    jobs: Mutex<()>,
    _lease: fs::File,
}

fn connect(state: &Path) -> Result<Connection> {
    let db = Connection::open(state.join("ledger.sqlite"))?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
        CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS revisions (id INTEGER PRIMARY KEY, body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS requests (id INTEGER PRIMARY KEY AUTOINCREMENT, agent INTEGER NOT NULL, request_id TEXT NOT NULL, body TEXT NOT NULL, UNIQUE(agent, request_id));
        CREATE TABLE IF NOT EXISTS captures (id TEXT PRIMARY KEY, body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS jobs (agent INTEGER NOT NULL, id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(agent,id));")?;
    Ok(db)
}
fn meta(db: &Connection, key: &str) -> Result<Option<String>> {
    Ok(db
        .query_row("SELECT value FROM meta WHERE key=?", [key], |r| r.get(0))
        .optional()?)
}
fn set_meta(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT INTO meta VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
fn records(db: &Connection) -> Result<Vec<Record>> {
    let mut stmt = db.prepare("SELECT body FROM requests ORDER BY id")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
fn save_record(db: &Connection, record: &Record) -> Result<()> {
    db.execute(
        "UPDATE requests SET body=? WHERE id=?",
        params![serde_json::to_string(record)?, record.operation as i64],
    )?;
    Ok(())
}
fn git(state: &Path, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
    let mut command = Command::new("/usr/bin/git");
    command
        .arg("--git-dir")
        .arg(state.join("objects.git"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    // Drain output concurrently so large immutable blobs cannot deadlock the pipe.
    let mut stdin = child.stdin.take().unwrap();
    std::thread::scope(|scope| {
        let write = scope.spawn(move || stdin.write_all(input));
        let output = child.wait_with_output()?;
        write.join().map_err(|_| "Git input worker failed")??;
        if !output.status.success() {
            return fail(format!(
                "Git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(output.stdout)
    })
}
fn object(state: &Path, args: &[&str], input: &[u8]) -> Result<String> {
    Ok(String::from_utf8(git(state, args, input)?)?.trim().into())
}
fn valid_path(path: &str) -> Result<()> {
    if !path.is_ascii() || path.is_empty() || path.contains(['\0', '\n', '\r', '\t', '\\', '"']) {
        return fail("unsupported source path");
    }
    let p = Path::new(path);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) || p.to_str() != Some(path) {
        return fail("source paths must be normalized relative paths");
    }
    for part in path.split('/') {
        if part.is_empty() || part.starts_with('.') || part.ends_with('.') {
            return fail("reserved or aliased path");
        }
    }
    Ok(())
}
fn read_file(live: &Path, path: &str) -> Result<Option<File>> {
    valid_path(path)?;
    let mut cursor = live.to_path_buf();
    let parts: Vec<_> = path.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        cursor.push(part);
        let metadata = match fs::symlink_metadata(&cursor) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if metadata.file_type().is_symlink() {
            return fail(format!("symlink boundary: {path}"));
        }
        if i + 1 < parts.len() {
            if !metadata.is_dir() {
                return fail("non-directory source ancestor");
            }
        } else {
            if !metadata.is_file() || metadata.nlink() != 1 {
                return fail(format!("unsupported file or hardlink: {path}"));
            }
            let mode = metadata.mode() & 0o7777;
            if mode != 0o644 && mode != 0o755 {
                return fail(format!("unsupported source mode: {path}"));
            }
            return Ok(Some(File {
                bytes: fs::read(cursor)?,
                executable: mode == 0o755,
            }));
        }
    }
    unreachable!()
}
fn paths(root: &Path, relative: &Path, output: &mut Vec<String>, ignore_git: bool) -> Result<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let name = entry.file_name();
        if ignore_git && relative.as_os_str().is_empty() && name == ".git" {
            continue;
        }
        let path = relative.join(name);
        let text = path.to_str().ok_or("non-UTF8 source path")?.to_string();
        valid_path(&text)?;
        if entry.file_type()?.is_dir() {
            paths(root, &path, output, ignore_git)?;
        } else {
            output.push(text);
        }
    }
    Ok(())
}
fn audit(live: &Path, capture: &Capture) -> Result<()> {
    let mut observed = Vec::new();
    paths(live, Path::new(""), &mut observed, true)?;
    for path in observed {
        if !capture.files.contains_key(&path) {
            return fail(format!("unenrolled source: {path}"));
        }
    }
    for (path, expected) in &capture.files {
        if read_file(live, path)? != expected.file {
            return fail(format!("unexplained source write: {path}"));
        }
    }
    Ok(())
}
fn build_tree(
    state: &Path,
    files: &BTreeMap<String, VersionedFile>,
    prefix: &str,
) -> Result<String> {
    let mut entries: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for (path, file) in files {
        if file.file.is_none() {
            continue;
        }
        let Some(tail) = path.strip_prefix(prefix) else {
            continue;
        };
        if let Some((dir, _)) = tail.split_once('/') {
            if !entries.contains_key(dir) {
                entries.insert(
                    dir.into(),
                    (
                        "040000".into(),
                        "tree".into(),
                        build_tree(state, files, &format!("{prefix}{dir}/"))?,
                    ),
                );
            }
        } else {
            let mode = if file.file.as_ref().unwrap().executable {
                "100755"
            } else {
                "100644"
            };
            entries.insert(
                tail.into(),
                (
                    mode.into(),
                    "blob".into(),
                    file.blob.clone().ok_or("missing blob")?,
                ),
            );
        }
    }
    let mut input = Vec::new();
    for (name, (mode, kind, hash)) in entries {
        input.extend_from_slice(format!("{mode} {kind} {hash}\t{name}\0").as_bytes());
    }
    object(state, &["mktree", "-z"], &input)
}
fn retain(state: &Path, capture: &Capture) -> Result<()> {
    // Verify exact trees, bytes and modes before retaining a reference or acknowledging capture.
    let mut expected = BTreeMap::new();
    for (path, file) in &capture.files {
        if let Some(source) = &file.file {
            let blob = file.blob.as_ref().ok_or("snapshot missing blob")?;
            if git(state, &["cat-file", "blob", blob], &[])? != source.bytes {
                return fail("corrupt snapshot blob");
            }
            expected.insert(
                path.clone(),
                (
                    if source.executable {
                        "100755"
                    } else {
                        "100644"
                    }
                    .to_string(),
                    blob.clone(),
                ),
            );
        }
    }
    let listing = git(state, &["ls-tree", "-rz", &capture.tree], &[])?;
    let mut actual = BTreeMap::new();
    for entry in listing.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let line = std::str::from_utf8(entry)?;
        let (info, path) = line.split_once('\t').ok_or("invalid tree entry")?;
        let parts: Vec<_> = info.split(' ').collect();
        if parts.len() != 3 || parts[1] != "blob" {
            return fail("unsupported retained tree");
        }
        actual.insert(path.into(), (parts[0].into(), parts[2].into()));
    }
    if expected != actual {
        return fail("snapshot tree mismatch");
    }
    let reference = format!("refs/falinks/trees/{}", capture.tree);
    match object(state, &["show-ref", "--verify", "--hash", &reference], &[]) {
        Ok(existing) if existing == capture.tree => {}
        Ok(_) => return fail("retention reference mismatch"),
        Err(_) => {
            git(state, &["update-ref", &reference, &capture.tree], &[])?;
        }
    }
    Ok(())
}
fn install(live: &Path, path: &str, file: &Option<File>) -> Result<()> {
    // Revalidate ancestors; the supported boundary excludes concurrent unmediated writers.
    read_file(live, path)?;
    let target = live.join(path);
    if let Some(file) = file {
        fs::create_dir_all(target.parent().ok_or("missing parent")?)?;
        let staging = target
            .parent()
            .unwrap()
            .join(format!(".falinks-install-{}", nonce()?));
        let result = (|| -> Result<()> {
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(if file.executable { 0o755 } else { 0o644 })
                .open(&staging)?;
            out.write_all(&file.bytes)?;
            out.set_permissions(fs::Permissions::from_mode(if file.executable {
                0o755
            } else {
                0o644
            }))?;
            out.sync_all()?;
            fs::rename(&staging, &target)?;
            Ok(())
        })();
        if staging.exists() {
            let _ = fs::remove_file(staging);
        }
        result
    } else {
        match fs::remove_file(target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

impl Engine {
    pub fn open(live: &Path, state: &Path, enrolled: &[&str]) -> Result<Self> {
        let live = fs::canonicalize(live)?;
        fs::create_dir_all(state)?;
        let state = fs::canonicalize(state)?;
        if state.starts_with(&live) || live.starts_with(&state) {
            return fail("controller storage must be separate from source");
        }
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(state.join("writer.lock"))?;
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return fail("engine writer already owned");
        }
        let db = connect(&state)?;
        let identity: Identity;
        let completed: Capture;
        if let Some(stored) = meta(&db, "identity")? {
            identity = serde_json::from_str(&stored)?;
            if meta(&db, "live")?.as_deref() != live.to_str() {
                return fail("workspace identity mismatch");
            }
            let current: u64 = meta(&db, "completed")?
                .ok_or("missing completed pointer")?
                .parse()?;
            completed = serde_json::from_str(&db.query_row(
                "SELECT body FROM revisions WHERE id=?",
                [current as i64],
                |r| r.get::<_, String>(0),
            )?)?;
            let expected: Vec<_> = enrolled.iter().map(|p| p.to_string()).collect();
            if expected.len() != completed.files.len()
                || expected.iter().any(|p| !completed.files.contains_key(p))
            {
                return fail("enrollment changed; explicit reset required");
            }
            for body in db
                .prepare("SELECT body FROM revisions")?
                .query_map([], |r| r.get::<_, String>(0))?
            {
                retain(&state, &serde_json::from_str::<Capture>(&body?)?)?;
            }
        } else {
            if state.join("objects.git").exists() {
                return fail("unrecorded controller storage; preserve for reconciliation");
            }
            let output = Command::new("/usr/bin/git")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .args(["init", "--bare"])
                .arg(state.join("objects.git"))
                .output()?;
            if !output.status.success() {
                return fail("cannot initialize Git storage");
            }
            identity = Identity {
                workspace: nonce()?,
                credentials: [nonce()?, nonce()?],
            };
            let mut files = BTreeMap::new();
            for path in enrolled {
                valid_path(path)?;
                if files.keys().any(|other: &String| {
                    other.eq_ignore_ascii_case(path)
                        || other.starts_with(&format!("{path}/"))
                        || path.starts_with(&format!("{other}/"))
                }) {
                    return fail("duplicate, prefix or case alias enrollment");
                }
                let file = read_file(&live, path)?;
                let blob = file
                    .as_ref()
                    .map(|f| object(&state, &["hash-object", "-w", "--stdin"], &f.bytes))
                    .transpose()?;
                files.insert(
                    path.to_string(),
                    VersionedFile {
                        version: 0,
                        file,
                        blob,
                    },
                );
            }
            completed = Capture {
                workspace: identity.workspace.clone(),
                revision: 0,
                tree: build_tree(&state, &files, "")?,
                files,
            };
            audit(&live, &completed)?;
            retain(&state, &completed)?;
            let tx = db.unchecked_transaction()?;
            set_meta(&tx, "identity", &serde_json::to_string(&identity)?)?;
            set_meta(&tx, "live", live.to_str().ok_or("non-UTF8 workspace")?)?;
            set_meta(&tx, "completed", "0")?;
            set_meta(&tx, "published", "0")?;
            tx.execute(
                "INSERT INTO revisions VALUES(0,?)",
                [serde_json::to_string(&completed)?],
            )?;
            tx.commit()?;
        }
        let engine = Self {
            live,
            state,
            identity,
            writer: Mutex::new(db),
            completed: RwLock::new(completed),
            jobs: Mutex::new(()),
            _lease: lease,
        };
        engine.recover()?;
        Ok(engine)
    }
    /// Host provisioning only: never expose these secrets as an agent tool.
    pub fn credentials(&self) -> &[String; 2] {
        &self.identity.credentials
    }
    pub fn authenticate(&self, agent: usize, credential: &str) -> Result<Client> {
        if self
            .identity
            .credentials
            .get(agent)
            .is_none_or(|c| c != credential)
        {
            return fail("authentication failed");
        }
        Ok(Client {
            agent,
            credential: credential.into(),
            workspace: self.identity.workspace.clone(),
        })
    }
    fn authorize(&self, client: &Client) -> Result<()> {
        if client.workspace != self.identity.workspace
            || self.identity.credentials.get(client.agent) != Some(&client.credential)
        {
            return fail("unauthorized client/workspace");
        }
        Ok(())
    }
    pub fn capture(&self) -> Result<Capture> {
        let capture = self
            .completed
            .read()
            .map_err(|_| "completed lock poisoned")?
            .clone();
        retain(&self.state, &capture)?;
        let db = connect(&self.state)?;
        db.execute(
            "INSERT INTO captures VALUES(?,?)",
            params![nonce()?, serde_json::to_string(&capture)?],
        )?;
        Ok(capture)
    }
    pub fn published(&self) -> Result<Capture> {
        let db = connect(&self.state)?;
        let revision = meta(&db, "published")?.ok_or("missing published pointer")?;
        Ok(serde_json::from_str(&db.query_row(
            "SELECT body FROM revisions WHERE id=?",
            [revision],
            |r| r.get::<_, String>(0),
        )?)?)
    }
    pub fn history(&self) -> Result<Vec<Record>> {
        records(&connect(&self.state)?)
    }
    pub fn request(&self, client: &Client, id: &str) -> Result<Option<Record>> {
        self.authorize(client)?;
        let db = connect(&self.state)?;
        let body = db
            .query_row(
                "SELECT body FROM requests WHERE agent=? AND request_id=?",
                params![client.agent as i64, id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        body.map(|b| Ok(serde_json::from_str(&b)?)).transpose()
    }
    pub fn apply(&self, client: &Client, request: Request) -> Result<Outcome> {
        self.apply_inner(client, request, false, &mut |_| Ok(()))
    }
    pub fn retry(&self, client: &Client, request: Request) -> Result<Outcome> {
        self.apply_inner(client, request, true, &mut |_| Ok(()))
    }
    fn apply_inner(
        &self,
        client: &Client,
        request: Request,
        retry: bool,
        observer: &mut dyn FnMut(Phase) -> Result<()>,
    ) -> Result<Outcome> {
        self.authorize(client)?;
        if request.id.is_empty() {
            return fail("request ID required");
        }
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let before = self
            .completed
            .read()
            .map_err(|_| "completed lock poisoned")?
            .clone();
        let existing = db
            .query_row(
                "SELECT body FROM requests WHERE agent=? AND request_id=?",
                params![client.agent as i64, request.id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        let mut record: Record;
        if let Some(body) = existing {
            record = serde_json::from_str(&body)?;
            if record.request != request {
                return fail("request ID reused with changed contents");
            }
            if let Some(outcome) = &record.outcome {
                if !retry || !matches!(outcome, Outcome::Interrupted { .. }) {
                    return Ok(outcome.clone());
                }
            } else {
                return fail("incomplete request requires restart reconciliation");
            }
            if record.installation_pending || meta(&db, "halted")?.is_some() {
                return fail("restart reconciliation required before explicit retry");
            }
            record.attempts.push(Attempt {
                before: record.before.clone(),
                proposed: record.proposed.clone(),
                outcome: record.outcome.clone(),
            });
            record.before = before.clone();
            record.proposed = None;
            record.outcome = None;
            save_record(&db, &record)?;
        } else {
            record = Record {
                operation: 0,
                attempts: Vec::new(),
                installation_pending: false,
                agent: client.agent,
                session: format!("{}:client:{}", self.identity.workspace, client.agent),
                request: request.clone(),
                before: before.clone(),
                proposed: None,
                outcome: None,
            };
            let tx = db.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO requests(agent,request_id,body) VALUES(?,?,?)",
                params![
                    client.agent as i64,
                    request.id,
                    serde_json::to_string(&record)?
                ],
            )?;
            record.operation = tx.last_insert_rowid() as u64;
            save_record(&tx, &record)?;
            tx.commit()?;
        }
        if let Some(reason) = meta(&db, "halted")? {
            return self.finish(&db, &mut record, Outcome::Rejected { reason });
        }
        if let Err(error) = audit(&self.live, &before) {
            let reason = error.to_string();
            self.save_incident(&db, &reason)?;
            return self.finish(&db, &mut record, Outcome::Rejected { reason });
        }
        let invalid = request.output.is_empty()
            || request
                .expected
                .keys()
                .any(|p| !before.files.contains_key(p))
            || request
                .output
                .keys()
                .any(|p| !request.expected.contains_key(p));
        if invalid {
            return self.finish(
                &db,
                &mut record,
                Outcome::Rejected {
                    reason: "all inputs/outputs must be enrolled with explicit freshness".into(),
                },
            );
        }
        let conflicts = request
            .expected
            .iter()
            .filter_map(|(path, expected)| {
                let current = &before.files[path];
                (current.version != *expected).then(|| {
                    (
                        path.clone(),
                        Conflict {
                            expected: *expected,
                            current: current.clone(),
                        },
                    )
                })
            })
            .collect::<BTreeMap<_, _>>();
        if !conflicts.is_empty() {
            return self.finish(&db, &mut record, Outcome::Stale { conflicts });
        }
        let result = (|| -> Result<Capture> {
            observer(Phase::BeforeRetention)?;
            let mut proposed = before.clone();
            proposed.revision = record.operation;
            for (path, file) in &request.output {
                let blob = file
                    .as_ref()
                    .map(|f| object(&self.state, &["hash-object", "-w", "--stdin"], &f.bytes))
                    .transpose()?;
                proposed.files.insert(
                    path.clone(),
                    VersionedFile {
                        version: record.operation,
                        file: file.clone(),
                        blob,
                    },
                );
            }
            proposed.tree = build_tree(&self.state, &proposed.files, "")?;
            retain(&self.state, &proposed)?;
            record.proposed = Some(proposed.clone());
            record.installation_pending = true;
            save_record(&db, &record)?;
            observer(Phase::Retained)?;
            for (path, file) in &request.output {
                install(&self.live, path, file)?;
                observer(Phase::Installed(path.clone()))?;
            }
            audit(&self.live, &proposed)?;
            observer(Phase::BeforeCommit)?;
            Ok(proposed)
        })();
        let proposed = match result {
            Ok(proposed) => proposed,
            Err(error) => {
                // Leave exact before/proposed bytes and request recoverable; no automatic continuation.
                let reason = error.to_string();
                set_meta(
                    &db,
                    "halted",
                    &format!("interrupted operation {}: {reason}", record.operation),
                )?;
                return self.finish(&db, &mut record, Outcome::Interrupted { reason });
            }
        };
        let outcome = Outcome::Applied {
            revision: proposed.revision,
            tree: proposed.tree.clone(),
        };
        record.outcome = Some(outcome.clone());
        record.installation_pending = false;
        let mut completed_guard = self
            .completed
            .write()
            .map_err(|_| "completed lock poisoned")?;
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO revisions VALUES(?,?)",
            params![proposed.revision as i64, serde_json::to_string(&proposed)?],
        )?;
        set_meta(&tx, "completed", &proposed.revision.to_string())?;
        save_record(&tx, &record)?;
        tx.commit()?;
        *completed_guard = proposed;
        drop(completed_guard);
        observer(Phase::Committed)?;
        Ok(outcome)
    }
    fn finish(&self, db: &Connection, record: &mut Record, outcome: Outcome) -> Result<Outcome> {
        record.outcome = Some(outcome.clone());
        save_record(db, record)?;
        Ok(outcome)
    }
    fn recover(&self) -> Result<()> {
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        let completed = self
            .completed
            .read()
            .map_err(|_| "completed lock poisoned")?
            .clone();
        for mut record in records(&db)? {
            if let Some(proposed) = &record.proposed {
                retain(&self.state, proposed)?;
            }
            for attempt in &record.attempts {
                retain(&self.state, &attempt.before)?;
                if let Some(proposed) = &attempt.proposed {
                    retain(&self.state, proposed)?;
                }
            }
            if record.outcome.is_none() || record.installation_pending {
                if let Some(proposed) = &record.proposed {
                    retain(&self.state, proposed)?;
                    // Check the full source universe before restoring any; never guess attribution.
                    let mut observed = Vec::new();
                    paths(&self.live, Path::new(""), &mut observed, true)?;
                    if observed.iter().any(|p| !completed.files.contains_key(p)) {
                        return fail("unenrolled source during recovery; evidence retained");
                    }
                    for (path, file) in &completed.files {
                        if !record.request.output.contains_key(path)
                            && read_file(&self.live, path)? != file.file
                        {
                            return fail(
                                "unexplained unrelated source during recovery; evidence retained",
                            );
                        }
                    }
                    for path in record.request.output.keys() {
                        let current = read_file(&self.live, path)?;
                        if current != record.before.files[path].file
                            && current != proposed.files[path].file
                        {
                            return fail(format!(
                                "ambiguous interrupted source: {path}; evidence retained"
                            ));
                        }
                    }
                    for path in record.request.output.keys() {
                        install(&self.live, path, &record.before.files[path].file)?;
                    }
                }
                record.installation_pending = false;
                if record.outcome.is_none() {
                    record.outcome = Some(Outcome::Interrupted {
                        reason: "restart reconciled; explicit retry required".into(),
                    });
                }
                save_record(&db, &record)?;
            }
        }
        let mut stmt = db.prepare("SELECT body FROM jobs")?;
        let job_rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for body in job_rows {
            let mut job: Job = serde_json::from_str(&body?)?;
            if !job.finished {
                job.finished = true;
                job.error = Some(
                    "restart: ambiguous job completion; output retained, new job required".into(),
                );
                save_job(&db, &job)?;
            }
        }
        audit(&self.live, &completed)?;
        // Only known interrupted installations are cleared. Unexplained edits stay halted.
        if meta(&db, "halted")?.is_some_and(|r| r.starts_with("interrupted operation ")) {
            db.execute("DELETE FROM meta WHERE key='halted'", [])?;
        }
        Ok(())
    }
}

/// Host fault/barrier seam; never exposed to untrusted clients.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    BeforeRetention,
    Retained,
    Installed(String),
    BeforeCommit,
    Committed,
}
impl Engine {
    pub fn apply_observed(
        &self,
        client: &Client,
        request: Request,
        mut observer: impl FnMut(Phase) -> Result<()>,
    ) -> Result<Outcome> {
        self.apply_inner(client, request, false, &mut observer)
    }
}

/// Commands are host-enrolled, trusted, non-daemonizing tools; never accept arbitrary
/// executable/arguments from an agent. macOS Seatbelt constrains their writable output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSpec {
    pub id: String,
    pub revision: u64,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub program: String,
    pub args: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub agent: usize,
    pub spec: JobSpec,
    pub directory: PathBuf,
    pub request: Request,
    pub finished: bool,
    pub error: Option<String>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
fn save_job(db: &Connection, job: &Job) -> Result<()> {
    db.execute(
        "INSERT INTO jobs VALUES(?,?,?) ON CONFLICT(agent,id) DO UPDATE SET body=excluded.body",
        params![job.agent as i64, job.spec.id, serde_json::to_string(job)?],
    )?;
    Ok(())
}
impl Engine {
    pub fn job(&self, client: &Client, id: &str) -> Result<Option<Job>> {
        self.authorize(client)?;
        let db = connect(&self.state)?;
        let body = db
            .query_row(
                "SELECT body FROM jobs WHERE agent=? AND id=?",
                params![client.agent as i64, id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        body.map(|b| Ok(serde_json::from_str(&b)?)).transpose()
    }
    pub fn run_job(&self, client: &Client, spec: JobSpec) -> Result<Job> {
        self.authorize(client)?;
        // ponytail: one captured job at a time; edits/captures remain independent.
        let _jobs = self.jobs.lock().map_err(|_| "job lock poisoned")?;
        if let Some(job) = self.job(client, &spec.id)? {
            if job.spec != spec {
                return fail("job request ID reused with changed contents");
            }
            return Ok(job);
        }
        if spec.id.is_empty()
            || spec.inputs.is_empty()
            || spec.outputs.is_empty()
            || !Path::new(&spec.program).is_absolute()
        {
            return fail("job needs an ID, declared input/output and host executable");
        }
        let db = connect(&self.state)?;
        let capture: Capture = serde_json::from_str(&db.query_row(
            "SELECT body FROM revisions WHERE id=?",
            [spec.revision as i64],
            |r| r.get::<_, String>(0),
        )?)?;
        retain(&self.state, &capture)?;
        let mut expected = BTreeMap::new();
        for path in spec.inputs.iter().chain(spec.outputs.iter()) {
            let file = capture
                .files
                .get(path)
                .ok_or("unenrolled job input/output")?;
            expected.insert(path.clone(), file.version);
        }
        let directory = self.state.join("jobs").join(nonce()?);
        fs::create_dir_all(directory.join("input"))?;
        fs::create_dir_all(directory.join("output"))?;
        let mut job = Job {
            agent: client.agent,
            spec: spec.clone(),
            directory: directory.clone(),
            request: Request {
                id: format!("job:{}", spec.id),
                expected,
                output: BTreeMap::new(),
            },
            finished: false,
            error: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        save_job(&db, &job)?;
        let result = (|| -> Result<()> {
            for path in &spec.inputs {
                install(&directory.join("input"), path, &capture.files[path].file)?;
            }
            for path in &spec.outputs {
                install(&directory.join("output"), path, &capture.files[path].file)?;
            }
            self.execute_job(&job)?;
            let mut actual = Vec::new();
            paths(&directory.join("output"), Path::new(""), &mut actual, false)?;
            if actual.iter().any(|p| !spec.outputs.contains(p)) {
                return fail("job wrote undeclared output");
            }
            for path in &spec.outputs {
                job.request
                    .output
                    .insert(path.clone(), read_file(&directory.join("output"), path)?);
            }
            Ok(())
        })();
        let result = result.and_then(|()| {
            job.stdout = fs::read(directory.join("stdout"))?;
            job.stderr = fs::read(directory.join("stderr"))?;
            Ok(())
        });
        if result.is_err() {
            job.stdout = fs::read(directory.join("stdout")).unwrap_or_default();
            job.stderr = fs::read(directory.join("stderr")).unwrap_or_default();
        }
        job.finished = true;
        job.error = result.err().map(|e| e.to_string());
        save_job(&db, &job)?;
        Ok(job)
    }
    fn execute_job(&self, job: &Job) -> Result<()> {
        if !cfg!(target_os = "macos") {
            return fail("captured jobs require verified macOS Seatbelt; unsupported platform");
        }
        for system in [
            "/usr",
            "/bin",
            "/System",
            "/Library",
            "/opt",
            "/private/etc",
            "/private/var/db/dyld",
            "/dev",
        ] {
            if self.live.starts_with(system) || self.state.starts_with(system) {
                return fail("source/controller overlaps captured-job system read boundary");
            }
        }
        let input = fs::canonicalize(job.directory.join("input"))?;
        let output = fs::canonicalize(job.directory.join("output"))?;
        // Serialize paths as quoted strings; prevent profile injection from storage paths.
        let quote = |p: &Path| -> Result<String> {
            let s = p.to_str().ok_or("non-UTF8 job directory")?;
            if s.contains(['\n', '\r']) {
                return fail("unsupported job directory");
            }
            Ok(serde_json::to_string(s)?)
        };
        let profile = format!(
            "(version 1) (allow default) (deny network*) (deny file-write*) (deny file-read*) (allow file-read* (literal \"/\") (subpath \"/usr\") (subpath \"/bin\") (subpath \"/System\") (subpath \"/Library\") (subpath \"/opt\") (subpath \"/private/etc\") (subpath \"/private/var/db/dyld\") (subpath \"/dev\") (subpath {}) (subpath {})) (allow file-write* (subpath {}))",
            quote(&input)?,
            quote(&output)?,
            quote(&output)?
        );
        fs::write(job.directory.join("sandbox.sb"), &profile)?;
        let stdout = fs::File::create(job.directory.join("stdout"))?;
        let stderr = fs::File::create(job.directory.join("stderr"))?;
        let mut child = Command::new("/usr/bin/sandbox-exec")
            .args(["-p", &profile, &job.spec.program])
            .args(&job.spec.args)
            .current_dir(&output)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TMPDIR", &output)
            .env("HOME", &output)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .process_group(0)
            .spawn()?;
        let group = child.id() as i32;
        // Persist running process identity as evidence; restart never adopts its output.
        fs::write(job.directory.join("process-group"), group.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                unsafe {
                    libc::kill(-group, libc::SIGKILL);
                }
                child.wait()?;
                return fail("job timed out; output retained");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        // Trusted tools must keep descendants in this group and join them before exit.
        // A surviving descendant makes completion ambiguous; kill the group, retain output.
        if unsafe { libc::kill(-group, 0) } == 0 {
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
            return fail("ambiguous completion: surviving job descendants");
        }
        if !status.success() {
            return fail(format!("captured job failed: {status}"));
        }
        Ok(())
    }
    pub fn apply_job(&self, client: &Client, id: &str) -> Result<Outcome> {
        let job = self.job(client, id)?.ok_or("unknown captured job")?;
        if !job.finished || job.error.is_some() {
            return fail(format!(
                "job unavailable: {:?}; evidence at {}",
                job.error,
                job.directory.display()
            ));
        }
        self.apply(client, job.request)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObservedFile {
    pub mode: u32,
    pub kind: String,
    pub bytes: Option<Vec<u8>>,
    pub symlink: Option<PathBuf>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Incident {
    pub reason: String,
    pub files: BTreeMap<String, ObservedFile>,
}
fn observe(root: &Path, relative: &Path, files: &mut BTreeMap<String, ObservedFile>) -> Result<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        if relative.as_os_str().is_empty() && entry.file_name() == ".git" {
            continue;
        }
        let path = relative.join(entry.file_name());
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() {
            observe(root, &path, files)?;
        } else {
            files.insert(
                path.to_string_lossy().into(),
                ObservedFile {
                    mode: metadata.mode(),
                    kind: if metadata.is_file() {
                        "file"
                    } else if metadata.file_type().is_symlink() {
                        "symlink"
                    } else {
                        "special"
                    }
                    .into(),
                    bytes: if metadata.is_file() {
                        Some(fs::read(entry.path())?)
                    } else {
                        None
                    },
                    symlink: if metadata.file_type().is_symlink() {
                        Some(fs::read_link(entry.path())?)
                    } else {
                        None
                    },
                },
            );
        }
    }
    Ok(())
}
impl Engine {
    fn save_incident(&self, db: &Connection, reason: &str) -> Result<()> {
        // Halt first even when retaining diagnostic evidence encounters another storage error.
        set_meta(db, "halted", reason)?;
        let mut incident = Incident {
            reason: reason.into(),
            files: BTreeMap::new(),
        };
        observe(&self.live, Path::new(""), &mut incident.files)?;
        set_meta(db, "incident", &serde_json::to_string(&incident)?)?;
        Ok(())
    }
    pub fn incident(&self) -> Result<Option<Incident>> {
        meta(&connect(&self.state)?, "incident")?
            .map(|s| Ok(serde_json::from_str(&s)?))
            .transpose()
    }
}
