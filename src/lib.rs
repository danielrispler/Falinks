//! Host-owned, single-repository protocol. Clients have opaque authenticated handles.
//! See docs/engine.md for the supported boundary.
mod analysis;
mod contain;
mod coordination;
mod publication;
mod regroup;
pub use analysis::{Evidence, Language, Owner, Pin, Tool, Toolchain, Unit, sha256_file};
#[doc(hidden)]
pub use contain::helper_main;
pub use coordination::{
    Availability, Body, Cancel, Checkpoint, Condition, Decision, Event, Kind, Message, Obligation,
    Offer, Pending, Plan, Reply, Review, Scope, WaitOutcome, Work,
};
pub use publication::{
    Binding, Candidate, Check, CheckResult, Run, RunAttempt, RunKind, RunOutcome, Stage,
};
pub use regroup::{Action, ProposalState, Propose, RegroupingProposal, Response, Signals};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock, RwLock, RwLockReadGuard},
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
impl File {
    /// Filesystem permission bits, the only two modes a source file may have.
    fn mode(&self) -> u32 {
        if self.executable { 0o755 } else { 0o644 }
    }
    /// Git tree entry mode.
    fn git_mode(&self) -> &'static str {
        if self.executable { "100755" } else { "100644" }
    }
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
    /// The physical live workspace this revision belongs to; 0 is the primary root.
    #[serde(default)]
    pub space: usize,
    /// Every applied operation whose effect this revision contains, published or not.
    #[serde(default)]
    pub included: BTreeSet<u64>,
    /// Agents placed in this workspace when the revision was created; host-required
    /// members of a candidate come from here, so later regrouping cannot change them.
    #[serde(default)]
    pub group: BTreeSet<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    /// All file/package inputs, including every output path. Versions are occurrences.
    pub expected: BTreeMap<String, u64>,
    /// Complete file replacements; None deletes an enrolled file.
    pub output: BTreeMap<String, Option<File>>,
    /// Inline reviews, recorded before the gate and kept whatever the outcome.
    #[serde(default)]
    pub reviews: Vec<Review>,
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
    /// Related to registered work whose relevant change has not been reviewed.
    Unreviewed {
        obligations: BTreeMap<String, Obligation>,
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
    /// The publication this operation incorporated, resolving overlapping drafts.
    #[serde(default)]
    pub incorporates: Option<u64>,
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

/// Physical live workspaces and the agents placed in them. Changes only under the writer.
/// Readers borrow it through `Engine::layout`. A writer copies it out before calling host
/// code; the copy stays current because every layout write happens under the writer.
#[derive(Clone)]
pub(crate) struct Layout {
    pub(crate) roots: Vec<PathBuf>,
    /// None until a provisioned root first receives a workspace.
    pub(crate) completed: Vec<Option<Capture>>,
    pub(crate) placement: [usize; 2],
}
impl Layout {
    /// The agent's workspace.
    pub(crate) fn of(&self, agent: usize) -> usize {
        self.placement[agent]
    }
    /// Agents placed in a workspace.
    pub(crate) fn group(&self, space: usize) -> BTreeSet<usize> {
        (0..2).filter(|a| self.placement[*a] == space).collect()
    }
    pub(crate) fn current(&self, space: usize) -> Result<&Capture> {
        Ok(self
            .completed
            .get(space)
            .and_then(Option::as_ref)
            .ok_or("workspace has no completed revision")?)
    }
}

pub struct Engine {
    state: PathBuf,
    identity: Identity,
    // ponytail: one writer mutex; split only if short-operation throughput warrants it.
    writer: Mutex<Connection>,
    // Every `RwLock` field is probed by `tests::rwlocks_free`; add new ones there.
    layout: RwLock<Layout>,
    jobs: Mutex<()>,
    analysis: RwLock<Vec<Toolchain>>,
    checks: RwLock<Vec<publication::Check>>,
    denied_roots: RwLock<Vec<PathBuf>>,
    helper: RwLock<Option<PathBuf>>,
    job_timeout: RwLock<std::time::Duration>,
    // One reusable validation workspace; its lease serializes validation runs.
    validation: Mutex<()>,
    signal: std::sync::Arc<coordination::Signal>,
    leases: Mutex<Vec<WriterLease>>,
    faults: Option<FaultHook>,
}

/// A ledger connection. The schema was set up when the engine opened, and write-ahead
/// logging persists in the database file; synchronous mode is per connection.
fn connect(state: &Path) -> Result<Connection> {
    let db = Connection::open(state.join("ledger.sqlite"))?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    db.execute_batch("PRAGMA synchronous=FULL;")?;
    Ok(db)
}
/// Once, when the engine opens: journal mode and every table, so older ledgers gain new tables.
fn schema(db: &Connection) -> Result<()> {
    db.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS revisions (id INTEGER PRIMARY KEY, body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS requests (id INTEGER PRIMARY KEY AUTOINCREMENT, agent INTEGER NOT NULL, request_id TEXT NOT NULL, body TEXT NOT NULL, UNIQUE(agent, request_id));
        CREATE TABLE IF NOT EXISTS incidents (id INTEGER PRIMARY KEY, body TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS captures (id TEXT PRIMARY KEY, body TEXT NOT NULL, recipient INTEGER);
        CREATE TABLE IF NOT EXISTS jobs (agent INTEGER NOT NULL, id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(agent,id));
        CREATE TABLE IF NOT EXISTS spaces (id INTEGER PRIMARY KEY, root TEXT NOT NULL UNIQUE, completed INTEGER);")?;
    coordination::tables(db)?;
    publication::tables(db)?;
    regroup::tables(db)
}
fn meta(db: &Connection, key: &str) -> Result<Option<String>> {
    Ok(db
        .query_row("SELECT value FROM meta WHERE key=?", [key], |r| r.get(0))
        .optional()?)
}
/// The single JSON column of the row `sql` selects, if any.
pub(crate) fn one<T: DeserializeOwned>(
    db: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Option<T>> {
    db.query_row(sql, params, |r| r.get::<_, String>(0))
        .optional()?
        .map(|b| Ok(serde_json::from_str(&b)?))
        .transpose()
}
/// The stored body of every row `sql` selects, in its order. `sql` selects one JSON
/// column; `json_array(seq, json(body))` pairs a body with its key.
pub(crate) fn all<T: DeserializeOwned>(
    db: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<T>> {
    let mut stmt = db.prepare(sql)?;
    let rows = stmt.query_map(params, |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
/// A JSON meta value, if set.
pub(crate) fn meta_value<T: DeserializeOwned>(db: &Connection, key: &str) -> Result<Option<T>> {
    one(db, "SELECT value FROM meta WHERE key=?", [key])
}
fn set_meta(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT INTO meta VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
fn records(db: &Connection) -> Result<Vec<Record>> {
    all(db, "SELECT body FROM requests ORDER BY id", [])
}
fn record(db: &Connection, operation: u64) -> Result<Record> {
    Ok(one(
        db,
        "SELECT body FROM requests WHERE id=?",
        [operation as i64],
    )?
    .ok_or("unknown operation")?)
}
/// The record of a client's request ID, if it was ever submitted.
fn request_record(db: &Connection, agent: usize, id: &str) -> Result<Option<Record>> {
    one(
        db,
        "SELECT body FROM requests WHERE agent=? AND request_id=?",
        params![agent as i64, id],
    )
}
pub(crate) fn revision(db: &Connection, revision: u64) -> Result<Capture> {
    Ok(one(
        db,
        "SELECT body FROM revisions WHERE id=?",
        [revision as i64],
    )?
    .ok_or("unknown completed revision")?)
}
/// Revision numbers are global across workspaces, so each names one occurrence.
pub(crate) fn next_revision(db: &Connection) -> Result<u64> {
    let max: i64 = db.query_row("SELECT COALESCE(MAX(id), 0) FROM revisions", [], |r| {
        r.get(0)
    })?;
    Ok(max as u64 + 1)
}
pub(crate) fn placement(db: &Connection) -> Result<[usize; 2]> {
    Ok(meta_value(db, "placement")?.unwrap_or_default())
}
fn save_record(db: &Connection, record: &Record) -> Result<()> {
    db.execute(
        "UPDATE requests SET body=? WHERE id=?",
        params![serde_json::to_string(record)?, record.operation as i64],
    )?;
    Ok(())
}
/// Git from the host's `PATH`, resolved once per process. 2.32 is the first release
/// that honours `GIT_CONFIG_GLOBAL`, which isolates storage from user configuration.
fn git_program() -> Result<&'static Path> {
    static GIT: OnceLock<std::result::Result<PathBuf, String>> = OnceLock::new();
    let find = || -> Result<PathBuf> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let git = std::env::split_paths(&path)
            .map(|dir| dir.join("git"))
            .find(|git| fs::metadata(git).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0))
            .ok_or("Git not found on PATH")?;
        let git = fs::canonicalize(git)?;
        let output = isolated_git(&git).arg("--version").output()?;
        let text = String::from_utf8_lossy(&output.stdout);
        if !git_version_supported(&text) {
            return fail(format!(
                "{} reports {:?}; Git 2.32 or newer is required",
                git.display(),
                text.trim()
            ));
        }
        Ok(git)
    };
    match GIT.get_or_init(|| find().map_err(|e| e.to_string())) {
        Ok(git) => Ok(git),
        Err(error) => fail(error.clone()),
    }
}
fn git_version_supported(text: &str) -> bool {
    let mut numbers = text
        .trim()
        .strip_prefix("git version ")
        .unwrap_or("")
        .split(['.', ' '])
        .map(|n| n.parse::<u32>().ok());
    matches!((numbers.next(), numbers.next()), (Some(Some(major)), Some(Some(minor))) if (major, minor) >= (2, 32))
}
/// Git with a cleared environment and no system or user configuration.
fn isolated_git(git: &Path) -> Command {
    let mut command = Command::new(git);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    command
}
/// Every storage Git call, including creating the bare store.
fn git(state: &Path, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
    let mut command = isolated_git(git_program()?);
    command
        .arg("--git-dir")
        .arg(state.join("objects.git"))
        .args(args)
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
/// Validates a source path's ancestors and metadata without reading its bytes.
/// Returns the file's location and executable bit, or `None` when it is absent.
fn check_path(live: &Path, path: &str) -> Result<Option<(PathBuf, bool)>> {
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
            // Supported only when the mode is exactly the one the file would derive.
            let file = File {
                bytes: Vec::new(),
                executable: mode & 0o111 != 0,
            };
            if file.mode() != mode {
                return fail(format!("unsupported source mode: {path}"));
            }
            return Ok(Some((cursor, file.executable)));
        }
    }
    unreachable!()
}
fn read_file(live: &Path, path: &str) -> Result<Option<File>> {
    check_path(live, path)?
        .map(|(location, executable)| {
            Ok(File {
                bytes: fs::read(location)?,
                executable,
            })
        })
        .transpose()
}
/// A live root's Git storage and writer lease: present at its top level, never source.
const GIT_STORAGE: &str = ".git";
const WRITER_LEASE: &str = ".falinks-writer.lock";
fn reserved(relative: &Path, name: &std::ffi::OsStr) -> bool {
    relative.as_os_str().is_empty() && (name == GIT_STORAGE || name == WRITER_LEASE)
}
/// Every source path under `root`; a live root skips its reserved names.
fn source_paths(root: &Path, live: bool) -> Result<Vec<String>> {
    fn walk(root: &Path, relative: &Path, live: bool, output: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let name = entry.file_name();
            if live && reserved(relative, &name) {
                continue;
            }
            let path = relative.join(name);
            let text = path.to_str().ok_or("non-UTF8 source path")?.to_string();
            valid_path(&text)?;
            if entry.file_type()?.is_dir() {
                walk(root, &path, live, output)?;
            } else {
                output.push(text);
            }
        }
        Ok(())
    }
    let mut output = Vec::new();
    walk(root, Path::new(""), live, &mut output)?;
    Ok(output)
}
fn audit(live: &Path, capture: &Capture) -> Result<()> {
    for path in source_paths(live, true)? {
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
/// Recovery of an interrupted installation, before any restore: every source path must
/// match `before` or `after` exactly, so attribution is never guessed. `None` is an empty
/// root. Returns the paths that reached `after` and must be put back.
fn audit_interrupted(
    root: &Path,
    before: Option<&Capture>,
    after: &Capture,
) -> Result<Vec<String>> {
    if let Some(path) = source_paths(root, true)?
        .into_iter()
        .find(|p| !after.files.contains_key(p))
    {
        return fail(format!(
            "unenrolled source during recovery: {path}; evidence retained"
        ));
    }
    let mut changed = Vec::new();
    for (path, file) in &after.files {
        let current = read_file(root, path)?;
        if current.as_ref() != before.and_then(|b| b.files[path].file.as_ref()) {
            if current != file.file {
                return fail(format!(
                    "ambiguous interrupted source: {path}; evidence retained"
                ));
            }
            changed.push(path.clone());
        }
    }
    Ok(changed)
}
/// Puts audited paths back to `before`; `None` removes them.
fn restore(root: &Path, before: Option<&Capture>, paths: &[String]) -> Result<()> {
    for path in paths {
        match before {
            Some(before) => install(root, path, &before.files[path].file)?,
            None => install(root, path, &None)?,
        }
    }
    Ok(())
}
/// Paths an operation installs: its outputs plus published files an incorporation brought in.
fn installed(before: &Capture, proposed: &Capture, request: &Request) -> BTreeSet<String> {
    proposed
        .files
        .iter()
        .filter(|(p, f)| before.files[*p].file != f.file || request.output.contains_key(*p))
        .map(|(p, _)| p.clone())
        .collect()
}
/// A provisioned root that has not yet received a workspace holds no source.
pub(crate) fn ensure_empty(root: &Path) -> Result<()> {
    match source_paths(root, true)?.first() {
        Some(path) => fail(format!("unexplained source write: {path}")),
        None => Ok(()),
    }
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
            entries.insert(
                tail.into(),
                (
                    file.file.as_ref().unwrap().git_mode().into(),
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
    let mut blobs = Vec::new();
    let mut input = String::new();
    for (path, file) in &capture.files {
        if let Some(source) = &file.file {
            let blob = file.blob.as_ref().ok_or("snapshot missing blob")?;
            input.push_str(blob);
            input.push('\n');
            blobs.push((blob, &source.bytes));
            expected.insert(path.clone(), (source.git_mode().to_string(), blob.clone()));
        }
    }
    // One batch process answers every blob, in input order: `<oid> blob <size>\n<bytes>\n`.
    let output = git(state, &["cat-file", "--batch"], input.as_bytes())?;
    let mut rest = output.as_slice();
    for (blob, bytes) in blobs {
        let end = rest
            .iter()
            .position(|b| *b == b'\n')
            .ok_or("truncated blob batch")?;
        let header = std::str::from_utf8(&rest[..end])?;
        rest = &rest[end + 1..];
        let size = match header.split(' ').collect::<Vec<_>>()[..] {
            [oid, "blob", size] if oid == blob => size.parse::<usize>()?,
            _ => return fail(format!("missing snapshot blob: {header}")),
        };
        if rest.get(..size) != Some(bytes.as_slice()) || rest.get(size) != Some(&b'\n') {
            return fail("corrupt snapshot blob");
        }
        rest = &rest[size + 1..];
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
    check_path(live, path)?;
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
                .mode(file.mode())
                .open(&staging)?;
            out.write_all(&file.bytes)?;
            out.set_permissions(fs::Permissions::from_mode(file.mode()))?;
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

struct WriterLease(fs::File);
impl Drop for WriterLease {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // Unlock the shared open-file description, including any pre-exec fork copies.
        // SAFETY: the fd is owned by this open File for the whole call; flock has no memory effects.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
fn writer_lease(path: &Path) -> Result<WriterLease> {
    use std::os::fd::AsRawFd;
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = lease.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return fail("invalid writer lease path");
    }
    // SAFETY: the fd is owned by `lease`, which outlives the call; flock has no memory effects.
    if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return fail("engine writer already owned");
    }
    Ok(WriterLease(lease))
}

impl Engine {
    pub fn open(live: &Path, state: &Path, enrolled: &[&str]) -> Result<Self> {
        Self::opening(live, state, enrolled, None)
    }
    /// Host tests only: `hook` runs at each durable step of transitions, synchronization
    /// and restart recovery, with no `RwLock` guard held. An error stops the engine there
    /// as a real failure at that step would.
    pub fn open_with_faults(
        live: &Path,
        state: &Path,
        enrolled: &[&str],
        hook: FaultHook,
    ) -> Result<Self> {
        Self::opening(live, state, enrolled, Some(hook))
    }
    fn opening(
        live: &Path,
        state: &Path,
        enrolled: &[&str],
        faults: Option<FaultHook>,
    ) -> Result<Self> {
        git_program()?;
        let live = fs::canonicalize(live)?;
        fs::create_dir_all(state)?;
        let state = fs::canonicalize(state)?;
        if state.starts_with(&live) || live.starts_with(&state) {
            return fail("controller storage must be separate from source");
        }
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
        let source_lease = writer_lease(&live.join(WRITER_LEASE))?;
        let state_lease = writer_lease(&state.join("writer.lock"))?;
        let db = connect(&state)?;
        schema(&db)?;
        let identity: Identity;
        let mut leases = vec![source_lease, state_lease];
        let mut layout = Layout {
            roots: vec![live.clone()],
            completed: Vec::new(),
            placement: placement(&db)?,
        };
        if let Some(stored) = meta_value(&db, "identity")? {
            identity = stored;
            if meta(&db, "live")?.as_deref() != live.to_str() {
                return fail("workspace identity mismatch");
            }
            let spaces = db
                .prepare("SELECT root, completed FROM spaces ORDER BY id")?
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (i, (root, current)) in spaces.into_iter().enumerate() {
                let root = PathBuf::from(root);
                if i == 0 && root != live {
                    return fail("workspace identity mismatch");
                }
                if i > 0 {
                    leases.push(writer_lease(&root.join(WRITER_LEASE))?);
                    layout.roots.push(root);
                }
                layout
                    .completed
                    .push(current.map(|c| revision(&db, c as u64)).transpose()?);
            }
            let completed = layout.current(0)?;
            let expected: Vec<_> = enrolled.iter().map(|p| p.to_string()).collect();
            if expected.len() != completed.files.len()
                || expected.iter().any(|p| !completed.files.contains_key(p))
            {
                return fail("enrollment changed; explicit reset required");
            }
            for capture in all::<Capture>(&db, "SELECT body FROM revisions", [])? {
                retain(&state, &capture)?;
            }
        } else {
            if state.join("objects.git").exists() {
                return fail("unrecorded controller storage; preserve for reconciliation");
            }
            git(&state, &["init", "--bare"], &[])
                .map_err(|e| format!("cannot initialize Git storage: {e}"))?;
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
            let completed = Capture {
                workspace: identity.workspace.clone(),
                revision: 0,
                tree: build_tree(&state, &files, "")?,
                files,
                space: 0,
                included: BTreeSet::new(),
                group: BTreeSet::from([0, 1]),
            };
            audit(&live, &completed)?;
            retain(&state, &completed)?;
            let tx = db.unchecked_transaction()?;
            set_meta(&tx, "identity", &serde_json::to_string(&identity)?)?;
            set_meta(&tx, "live", live.to_str().ok_or("non-UTF8 workspace")?)?;
            tx.execute(
                "INSERT INTO spaces VALUES(0,?,0)",
                [live.to_str().ok_or("non-UTF8 workspace")?],
            )?;
            set_meta(&tx, "published", "0")?;
            tx.execute(
                "INSERT INTO revisions VALUES(0,?)",
                [serde_json::to_string(&completed)?],
            )?;
            tx.commit()?;
            layout.completed.push(Some(completed));
        }
        let engine = Self {
            state,
            identity,
            writer: Mutex::new(db),
            layout: RwLock::new(layout),
            jobs: Mutex::new(()),
            analysis: RwLock::new(Vec::new()),
            checks: RwLock::new(Vec::new()),
            denied_roots: RwLock::new(Vec::new()),
            helper: RwLock::new(None),
            job_timeout: RwLock::new(std::time::Duration::from_secs(30)),
            validation: Mutex::new(()),
            signal: coordination::Signal::new(),
            leases: Mutex::new(leases),
            faults,
        };
        engine.recover()?;
        publication::repair_mirror(&engine.state, &engine.published()?.tree)?;
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
        self.signal.presence(agent, true);
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
    /// Borrows the layout. Writers change it only under the writer lock, so a reader
    /// holding that lock must release this guard before `self.layout.write()`.
    pub(crate) fn layout(&self) -> Result<RwLockReadGuard<'_, Layout>> {
        Ok(self.layout.read().map_err(|_| "layout lock poisoned")?)
    }
    /// Host read of the primary workspace (space 0), where both agents start joined.
    /// Agents read only their own workspace, through `capture_for`.
    pub fn capture(&self) -> Result<Capture> {
        self.capture_space(0, None)
    }
    /// Captures the live workspace the client is currently placed in, given to that client.
    pub fn capture_for(&self, client: &Client) -> Result<Capture> {
        self.authorize(client)?;
        let space = self.layout()?.of(client.agent);
        self.capture_space(space, Some(client.agent))
    }
    /// Host-internal capture of the client's workspace, not given to the client: its
    /// revision can be offered but never authorizes the client's review.
    pub fn host_capture(&self, client: &Client) -> Result<Capture> {
        self.authorize(client)?;
        let space = self.layout()?.of(client.agent);
        self.capture_space(space, None)
    }
    /// Records each capture separately with its recipient; None is a host-internal capture.
    fn capture_space(&self, space: usize, recipient: Option<usize>) -> Result<Capture> {
        let capture = self.layout()?.current(space)?.clone();
        retain(&self.state, &capture)?;
        let db = connect(&self.state)?;
        db.execute(
            "INSERT INTO captures VALUES(?,?,?)",
            params![
                nonce()?,
                serde_json::to_string(&capture)?,
                recipient.map(|a| a as i64)
            ],
        )?;
        Ok(capture)
    }
    pub fn published(&self) -> Result<Capture> {
        let db = connect(&self.state)?;
        revision(&db, publication::published(&db)?)
    }
    pub fn history(&self) -> Result<Vec<Record>> {
        records(&connect(&self.state)?)
    }
    pub fn request(&self, client: &Client, id: &str) -> Result<Option<Record>> {
        self.authorize(client)?;
        request_record(&connect(&self.state)?, client.agent, id)
    }
    pub fn apply(&self, client: &Client, request: Request) -> Result<Outcome> {
        self.apply_inner(client, request, Mode::Apply, &mut |_| Ok(()))
    }
    pub fn retry(&self, client: &Client, request: Request) -> Result<Outcome> {
        self.apply_inner(client, request, Mode::Retry, &mut |_| Ok(()))
    }
    /// Brings the published state into the client's split workspace when its own
    /// unpublished drafts overlap the publication. `output` must resolve every overlap.
    pub fn incorporate(&self, client: &Client, request: Request) -> Result<Outcome> {
        self.apply_inner(client, request, Mode::Incorporate, &mut |_| Ok(()))
    }
    fn apply_inner(
        &self,
        client: &Client,
        request: Request,
        mode: Mode,
        observer: &mut dyn FnMut(Phase) -> Result<()>,
    ) -> Result<Outcome> {
        let outcome = self.apply_locked(client, request, mode, observer)?;
        self.boundary();
        Ok(outcome)
    }
    fn apply_locked(
        &self,
        client: &Client,
        request: Request,
        mode: Mode,
        observer: &mut dyn FnMut(Phase) -> Result<()>,
    ) -> Result<Outcome> {
        self.authorize(client)?;
        if request.id.is_empty() {
            return fail("request ID required");
        }
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        // Inline reviews stand even if the edit is then refused, as separate reviews would.
        // A known ID already recorded them on its first call.
        if request_record(&db, client.agent, &request.id)?.is_none() {
            self.record_reviews(&db, client, &request.reviews)?;
        }
        // Copied out: no layout guard is held while host observers run.
        let (space, live, before, group) = {
            let layout = self.layout()?;
            let space = layout.of(client.agent);
            let before = layout.current(space)?.clone();
            (
                space,
                layout.roots[space].clone(),
                before,
                layout.group(space),
            )
        };
        let mut record: Record;
        if let Some(existing) = request_record(&db, client.agent, &request.id)? {
            record = existing;
            if record.request != request {
                return fail("request ID reused with changed contents");
            }
            if let Some(outcome) = &record.outcome {
                if mode != Mode::Retry || !matches!(outcome, Outcome::Interrupted { .. }) {
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
                incorporates: None,
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
        if let Err(error) = audit(&live, &before) {
            let reason = error.to_string();
            self.save_incident(&db, &self.layout()?.roots, &reason)?;
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
        let interrupted =
            |db: &Connection,
             record: &mut Record,
             error: Box<dyn std::error::Error + Send + Sync>| {
                // Leave exact before/proposed bytes and request recoverable; no automatic continuation.
                let reason = error.to_string();
                set_meta(
                    db,
                    "halted",
                    &format!("interrupted operation {}: {reason}", record.operation),
                )?;
                self.finish(db, record, Outcome::Interrupted { reason })
            };
        let base = if mode == Mode::Incorporate {
            let published = publication::published(&db)?;
            let (merged, overlaps) = regroup::merge(&db, &before, &revision(&db, published)?)?;
            let unresolved: Vec<_> = overlaps
                .iter()
                .filter(|p| !request.output.contains_key(*p))
                .collect();
            if before.included.is_superset(&merged.included) && overlaps.is_empty() {
                return self.finish(
                    &db,
                    &mut record,
                    Outcome::Rejected {
                        reason: "workspace already incorporates the published state".into(),
                    },
                );
            }
            if !unresolved.is_empty() {
                return self.finish(
                    &db,
                    &mut record,
                    Outcome::Rejected {
                        reason: format!(
                            "incorporation must resolve overlapping drafts: {unresolved:?}"
                        ),
                    },
                );
            }
            record.incorporates = Some(published);
            merged
        } else {
            before.clone()
        };
        let proposal = (|| -> Result<Capture> {
            let mut proposed = base.clone();
            proposed.revision = next_revision(&db)?;
            proposed.included.insert(record.operation);
            proposed.group = group;
            for (path, file) in &request.output {
                let blob = file
                    .as_ref()
                    .map(|f| object(&self.state, &["hash-object", "-w", "--stdin"], &f.bytes))
                    .transpose()?;
                proposed.files.insert(
                    path.clone(),
                    VersionedFile {
                        version: proposed.revision,
                        file: file.clone(),
                        blob,
                    },
                );
            }
            proposed.tree = build_tree(&self.state, &proposed.files, "")?;
            Ok(proposed)
        })();
        // Synchronized files that the request does not replace are installed too.
        let installs: BTreeMap<String, Option<File>> = proposal
            .as_ref()
            .map(|proposed| {
                installed(&before, proposed, &request)
                    .into_iter()
                    .map(|p| (p.clone(), proposed.files[&p].file.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let proposed = match proposal {
            Ok(proposed) => proposed,
            Err(error) => return interrupted(&db, &mut record, error),
        };
        let changed: BTreeSet<String> = installs.keys().cloned().collect();
        let change = match self.gate(&db, client, &before, &proposed, &changed) {
            Ok(Ok(change)) => change,
            Ok(Err(outcome)) => return self.finish(&db, &mut record, outcome),
            // Nothing installed yet: record a visible outcome rather than strand the request.
            Err(error) => {
                let reason = format!("coordination gate failed: {error}");
                return self.finish(&db, &mut record, Outcome::Rejected { reason });
            }
        };
        let result = (|| -> Result<Capture> {
            observer(Phase::BeforeRetention)?;
            retain(&self.state, &proposed)?;
            record.proposed = Some(proposed.clone());
            record.installation_pending = true;
            save_record(&db, &record)?;
            observer(Phase::Retained)?;
            for (path, file) in &installs {
                install(&live, path, file)?;
                observer(Phase::Installed(path.clone()))?;
            }
            audit(&live, &proposed)?;
            observer(Phase::BeforeCommit)?;
            Ok(proposed)
        })();
        let proposed = match result {
            Ok(proposed) => proposed,
            Err(error) => return interrupted(&db, &mut record, error),
        };
        let outcome = Outcome::Applied {
            revision: proposed.revision,
            tree: proposed.tree.clone(),
        };
        record.outcome = Some(outcome.clone());
        record.installation_pending = false;
        // Published files an incorporation brought in are changes for the client too.
        let synced: BTreeSet<String> = changed
            .into_iter()
            .filter(|p| !request.output.contains_key(p))
            .collect();
        let mut layout_guard = self.layout.write().map_err(|_| "layout lock poisoned")?;
        let committed = (|| -> Result<()> {
            let tx = db.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO revisions VALUES(?,?)",
                params![proposed.revision as i64, serde_json::to_string(&proposed)?],
            )?;
            tx.execute(
                "UPDATE spaces SET completed=? WHERE id=?",
                params![proposed.revision as i64, space as i64],
            )?;
            save_record(&tx, &record)?;
            let cause =
                self.record_change(&tx, client, &request, &proposed, &change, &layout_guard)?;
            if !synced.is_empty() {
                // Judged on the evidence the gate gathered for the same two captures.
                let view = change.for_paths(&before, &proposed, &synced);
                self.renew_for_view(&tx, client.agent, &view, proposed.revision, cause)?;
            }
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = committed {
            // The files are in place but uncommitted: restart restores the before state.
            drop(layout_guard);
            record.installation_pending = true;
            return interrupted(&db, &mut record, error);
        }
        layout_guard.completed[space] = Some(proposed);
        drop(layout_guard);
        self.signal.notify();
        observer(Phase::Committed)?;
        Ok(outcome)
    }
    /// Every provisioned root matches its completed revision; an unused root is empty.
    pub(crate) fn audit_all(&self, layout: &Layout) -> Result<()> {
        for (root, completed) in layout.roots.iter().zip(&layout.completed) {
            match completed {
                Some(completed) => audit(root, completed)?,
                None => ensure_empty(root)?,
            }
        }
        Ok(())
    }
    fn finish(&self, db: &Connection, record: &mut Record, outcome: Outcome) -> Result<Outcome> {
        record.outcome = Some(outcome.clone());
        save_record(db, record)?;
        Ok(outcome)
    }
    /// Three phases: check every live root, then restore, then update the journals. A
    /// failed check records an incident and changes no root and no journal.
    fn recover(&self) -> Result<()> {
        let db = self.writer.lock().map_err(|_| "writer lock poisoned")?;
        // Copied out: no layout guard is held while the fault hook runs.
        let layout = self.layout()?.clone();
        let mut records = records(&db)?;
        for record in &records {
            if let Some(proposed) = &record.proposed {
                retain(&self.state, proposed)?;
            }
            for attempt in &record.attempts {
                retain(&self.state, &attempt.before)?;
                if let Some(proposed) = &attempt.proposed {
                    retain(&self.state, proposed)?;
                }
            }
        }
        records.retain(|r| r.outcome.is_none() || r.installation_pending);
        let shifts = regroup::installing(self, &db)?;
        // Pending installations by root: (space, before, after); `None` is an empty root.
        let pending: Vec<(usize, Option<&Capture>, &Capture)> = records
            .iter()
            .filter_map(|r| Some((r.before.space, Some(&r.before), r.proposed.as_ref()?)))
            .chain(shifts.iter().flat_map(|(_, shift)| {
                shift
                    .moves
                    .iter()
                    .map(|c| (c.space, c.before.as_ref(), &c.after))
            }))
            .collect();
        // Phase 1: a pending root holds its before or after state path by path; every other
        // root holds its completed state, or nothing.
        let checked = (|| -> Result<Vec<_>> {
            let mut restores = Vec::new();
            for &(space, before, after) in &pending {
                // Restoring `before` must yield the completed state, once per root.
                if before != layout.completed[space].as_ref()
                    || pending.iter().filter(|p| p.0 == space).count() > 1
                {
                    return fail(format!(
                        "unexplained pending installation in workspace {space}"
                    ));
                }
                let changed = audit_interrupted(&layout.roots[space], before, after)?;
                restores.push((space, before, changed));
            }
            for (space, (root, completed)) in layout.roots.iter().zip(&layout.completed).enumerate()
            {
                if pending.iter().any(|p| p.0 == space) {
                    continue;
                }
                match completed {
                    Some(completed) => audit(root, completed)?,
                    None => ensure_empty(root)?,
                }
            }
            Ok(restores)
        })();
        let restores = match checked {
            Ok(restores) => restores,
            Err(error) => {
                self.save_incident(&db, &layout.roots, &error.to_string())?;
                return Err(error);
            }
        };
        // Phase 2.
        for (space, before, changed) in restores {
            for path in changed {
                restore(&layout.roots[space], before, std::slice::from_ref(&path))?;
                self.fault(Fault::Restored { space, path })?;
            }
        }
        // Phase 3: the journals.
        for mut record in records {
            record.installation_pending = false;
            if record.outcome.is_none() {
                record.outcome = Some(Outcome::Interrupted {
                    reason: "restart reconciled; explicit retry required".into(),
                });
            }
            save_record(&db, &record)?;
        }
        for (id, shift) in shifts {
            regroup::restored(&db, id, shift)?;
        }
        // Outside the phases: publication runs and captured jobs write no live root, and a
        // restart interrupts them whatever the roots contain.
        publication::recover(&db)?;
        for mut job in all::<Job>(&db, "SELECT body FROM jobs", [])? {
            if !job.finished {
                job.finished = true;
                job.error = Some(
                    "restart: ambiguous job completion; output retained, new job required".into(),
                );
                save_job(&db, &job)?;
            }
        }
        // Only known interrupted installations are cleared. Unexplained edits stay halted.
        if meta(&db, "halted")?.is_some_and(|r| {
            r.starts_with("interrupted operation ") || r.starts_with("interrupted transition ")
        }) {
            db.execute("DELETE FROM meta WHERE key='halted'", [])?;
        }
        if let Some(reason) = meta(&db, "halted")? {
            return fail(format!("engine halted for reconciliation: {reason}"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Apply,
    /// Explicit retry of an interrupted request.
    Retry,
    /// Also brings the published state into a split workspace.
    Incorporate,
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
/// A durable step of a transition, a synchronization or restart recovery, for the host
/// fault hook; never exposed to untrusted clients. `space` names a live root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The installation is recorded; no root is written yet.
    Journaled,
    Installed {
        space: usize,
        path: String,
    },
    /// A root holds its new state.
    Audited {
        space: usize,
    },
    /// Inside the commit transaction, before it commits.
    Commit,
    Committed,
    /// Restart recovery put one path back to its before state.
    Restored {
        space: usize,
        path: String,
    },
}
/// Receives the engine so it may read it, as a host on the same thread could.
pub type FaultHook = Box<dyn Fn(&Engine, Fault) -> Result<()> + Send + Sync>;
impl Engine {
    pub(crate) fn fault(&self, fault: Fault) -> Result<()> {
        self.faults
            .as_ref()
            .map_or(Ok(()), |hook| hook(self, fault))
    }
    pub fn apply_observed(
        &self,
        client: &Client,
        request: Request,
        mut observer: impl FnMut(Phase) -> Result<()>,
    ) -> Result<Outcome> {
        self.apply_inner(client, request, Mode::Apply, &mut observer)
    }
}

/// Commands are host-enrolled, trusted, non-daemonizing tools; never accept arbitrary
/// executable/arguments from an agent. They run as contained commands (see `contain`).
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
        one(
            &db,
            "SELECT body FROM jobs WHERE agent=? AND id=?",
            params![client.agent as i64, id],
        )
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
        let capture = revision(&db, spec.revision)?;
        if capture.space != self.layout()?.of(client.agent) {
            return fail("job must capture a revision of the client's workspace");
        }
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
                reviews: Vec::new(),
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
            if source_paths(&directory.join("output"), false)?
                .iter()
                .any(|p| !spec.outputs.contains(p))
            {
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
        let layout = self.layout()?;
        for system in contain::SYSTEM_READS {
            if layout.roots.iter().any(|r| r.starts_with(system)) || self.state.starts_with(system)
            {
                return fail("source/controller overlaps captured-job system read boundary");
            }
        }
        drop(layout);
        let input = fs::canonicalize(job.directory.join("input"))?;
        let output = fs::canonicalize(job.directory.join("output"))?;
        let status = contain::run(
            || self.helper(),
            &contain::Contained {
                evidence: &job.directory,
                program: &job.spec.program,
                args: &job.spec.args,
                cwd: &output,
                output: &output,
                env: &[("TMPDIR", &output), ("HOME", &output)],
                reads: contain::Reads::Only(vec![input.clone(), output.clone()]),
                timeout: *self
                    .job_timeout
                    .read()
                    .map_err(|_| "job timeout lock poisoned")?,
            },
        )?;
        if !status.success() {
            return fail(format!("captured job failed: {status}"));
        }
        Ok(())
    }
    /// Host configuration: the Linux containment helper (ADR 2). Defaults to
    /// `falinks-contain` beside the host executable; a missing helper fails closed.
    pub fn configure_containment(&self, helper: PathBuf) -> Result<()> {
        *self.helper.write().map_err(|_| "helper lock poisoned")? = Some(helper);
        Ok(())
    }
    /// Host configuration: how long a captured job may run before it is killed
    /// (default 30s).
    pub fn configure_job_timeout(&self, timeout: std::time::Duration) -> Result<()> {
        *self
            .job_timeout
            .write()
            .map_err(|_| "job timeout lock poisoned")? = timeout;
        Ok(())
    }
    fn helper(&self) -> Result<PathBuf> {
        match &*self.helper.read().map_err(|_| "helper lock poisoned")? {
            Some(helper) => Ok(helper.clone()),
            None => contain::default_helper(),
        }
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
        if reserved(relative, &entry.file_name()) {
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
    /// `roots` are the layout's live roots; the caller may hold the layout guard.
    fn save_incident(&self, db: &Connection, roots: &[PathBuf], reason: &str) -> Result<()> {
        // Halt first even when retaining diagnostic evidence encounters another storage error.
        set_meta(db, "halted", reason)?;
        let mut incident = Incident {
            reason: reason.into(),
            files: BTreeMap::new(),
        };
        for (space, root) in roots.iter().enumerate() {
            let mut files = BTreeMap::new();
            observe(root, Path::new(""), &mut files)?;
            // Paths outside the primary root are prefixed with their workspace.
            incident
                .files
                .extend(files.into_iter().map(|(path, file)| match space {
                    0 => (path, file),
                    _ => (format!("@{space}/{path}"), file),
                }));
        }
        let body = serde_json::to_string(&incident)?;
        let tx = db.unchecked_transaction()?;
        tx.execute("INSERT INTO incidents(body) VALUES(?)", [&body])?;
        set_meta(&tx, "incident", &body)?;
        tx.commit()?;
        Ok(())
    }
    pub fn incident(&self) -> Result<Option<Incident>> {
        Self::incident_at(&self.state)
    }
    pub fn incident_at(state: &Path) -> Result<Option<Incident>> {
        meta_value(
            &Connection::open_with_flags(
                state.join("ledger.sqlite"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?,
            "incident",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(state: &Path, sources: &[(&str, &[u8])]) -> Capture {
        let mut files = BTreeMap::new();
        for (path, bytes) in sources {
            let file = File {
                bytes: bytes.to_vec(),
                executable: false,
            };
            let blob = object(state, &["hash-object", "-w", "--stdin"], bytes).unwrap();
            files.insert(
                path.to_string(),
                VersionedFile {
                    version: 0,
                    file: Some(file),
                    blob: Some(blob),
                },
            );
        }
        Capture {
            workspace: "test".into(),
            revision: 0,
            tree: build_tree(state, &files, "").unwrap(),
            files,
            space: 0,
            included: BTreeSet::new(),
            group: BTreeSet::new(),
        }
    }

    #[test]
    fn retention_checks_every_blob_of_a_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path();
        git(state, &["init", "--bare"], &[]).unwrap();
        let capture = snapshot(
            state,
            &[("a.rs", b"a"), ("dir/b.rs", b""), ("c.rs", b"c\n")],
        );
        retain(state, &capture).unwrap();

        let mut corrupt = capture.clone();
        corrupt.files.get_mut("c.rs").unwrap().file = Some(File {
            bytes: b"x\n".to_vec(),
            executable: false,
        });
        assert!(retain(state, &corrupt).is_err());

        let mut missing = capture.clone();
        missing.files.get_mut("a.rs").unwrap().blob = Some("0".repeat(40));
        assert!(retain(state, &missing).is_err());
    }

    impl Engine {
        /// No guard on any `RwLock` is held. `try_write` never blocks, so a held guard fails
        /// an assertion instead of hanging. The operation's own mutexes may be held.
        fn rwlocks_free(&self) -> bool {
            self.layout.try_write().is_ok()
                && self.analysis.try_write().is_ok()
                && self.checks.try_write().is_ok()
                && self.denied_roots.try_write().is_ok()
                && self.helper.try_write().is_ok()
                && self.job_timeout.try_write().is_ok()
        }
    }

    // A host observer may read the engine on the same thread, where re-locking a std
    // `RwLock` may panic or deadlock. A plain re-entrant read usually succeeds and would not
    // catch the regression, so the probe checks that every guard was released.
    #[test]
    fn host_observers_run_with_every_rwlock_released() {
        let dir = tempfile::tempdir().unwrap();
        let (live, second) = (dir.path().join("live"), dir.path().join("second"));
        std::fs::create_dir_all(&live).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(live.join("a.rs"), "fn a() {}\n").unwrap();
        // The first commit fails, so a restart has a transition to restore.
        let faults = std::sync::Arc::new(Mutex::new(Vec::new()));
        let hook = || -> FaultHook {
            let faults = faults.clone();
            Box::new(move |engine, fault| {
                assert!(engine.rwlocks_free(), "guard held at {fault:?}");
                let mut faults = faults.lock().unwrap();
                let first_commit = fault == Fault::Commit && !faults.contains(&fault);
                faults.push(fault);
                if first_commit {
                    return Err("interrupt".into());
                }
                Ok(())
            })
        };
        let open = || Engine::open_with_faults(&live, &dir.path().join("state"), &["a.rs"], hook());
        let engine = open().unwrap();
        let alice = engine.authenticate(0, &engine.credentials()[0]).unwrap();
        let bob = engine.authenticate(1, &engine.credentials()[1]).unwrap();
        engine.add_workspace(&second).unwrap();
        let split = Propose {
            action: Action::Split,
            text: "split".into(),
            failing: false,
        };
        let proposal = engine.propose(&bob, split).unwrap();
        for client in [&alice, &bob] {
            engine
                .respond(client, &proposal.id, &proposal.context, Response::Agree)
                .unwrap();
        }
        drop(engine);
        let engine = open().unwrap();

        let base = engine.capture().unwrap();
        let request = Request {
            id: "a".into(),
            expected: BTreeMap::from([("a.rs".into(), base.files["a.rs"].version)]),
            output: BTreeMap::from([(
                "a.rs".into(),
                Some(File {
                    bytes: b"fn a() { 1; }\n".to_vec(),
                    executable: false,
                }),
            )]),
            reviews: Vec::new(),
        };
        let mut phases = Vec::new();
        let outcome = engine
            .apply_observed(&alice, request, |phase| {
                assert!(engine.rwlocks_free(), "guard held at {phase:?}");
                phases.push(phase);
                Ok(())
            })
            .unwrap();
        assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
        assert!(phases.contains(&Phase::Committed), "{phases:?}");

        let offer = Offer {
            id: "a".into(),
            task: "a".into(),
            revision: engine.capture().unwrap().revision,
            scope: vec!["a.rs".into()],
            text: "a".into(),
            supersedes: None,
            reviews: Vec::new(),
        };
        engine.offer(&alice, offer).unwrap();
        let check = Check {
            name: "true".into(),
            program: "/usr/bin/true".into(),
            args: Vec::new(),
        };
        engine.configure_checks(vec![check]).unwrap();
        let mut stages = Vec::new();
        let run = engine
            .validate_observed(|stage| {
                assert!(engine.rwlocks_free(), "guard held at {stage:?}");
                stages.push(stage);
                Ok(())
            })
            .unwrap()
            .unwrap();
        assert!(
            matches!(run.outcome, Some(RunOutcome::Published { .. })),
            "{run:?}"
        );
        assert!(stages.contains(&Stage::Accepted), "{stages:?}");
        // The split retried at the edit's boundary, then the publication reached `second`.
        let faults = faults.lock().unwrap();
        assert!(faults.contains(&Fault::Restored {
            space: 1,
            path: "a.rs".into()
        }));
        assert_eq!(
            faults.iter().filter(|f| **f == Fault::Committed).count(),
            2,
            "{faults:?}"
        );
    }
}
