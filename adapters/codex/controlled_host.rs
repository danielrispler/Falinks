use crate::{
    Result, file_hash, hash, require,
    runtime::{EngineOutcome, Notice},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub fn inventory(source: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(source)?;
    require(
        metadata.is_dir() && !metadata.is_symlink(),
        "unexpected source root",
    )?;
    fn scan(root: &Path, path: &Path, entries: &mut BTreeMap<String, Value>) -> Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        require(
            !metadata.is_symlink() && (metadata.is_file() || metadata.is_dir()),
            "unexpected source alias or file type",
        )?;
        if metadata.is_file() {
            require(metadata.nlink() == 1, "unexpected source hard link")?;
        }
        let name = if path == root {
            ".".to_owned()
        } else {
            path.strip_prefix(root)?.to_string_lossy().into_owned()
        };
        entries.insert(
            name,
            json!([
                metadata.mode(),
                if metadata.is_file() {
                    Some(file_hash(path)?)
                } else {
                    None
                }
            ]),
        );
        if metadata.is_dir() {
            for item in fs::read_dir(path)? {
                scan(root, &item?.path(), entries)?;
            }
        }
        Ok(())
    }
    // ponytail: bounded fixture inventory; production engine supplies its revision inventory.
    let mut entries = BTreeMap::new();
    scan(source, source, &mut entries)?;
    Ok(hash(&serde_json::to_vec(&entries)?))
}

pub struct ControlledHost {
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
    db: Connection,
    pub records: Vec<Value>,
}
impl ControlledHost {
    pub fn new(root: &Path) -> Result<Self> {
        let source = root.join("source");
        let target = source.join("source.txt");
        let db = Connection::open(root.join("controller/host.sqlite"))?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS state(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE IF NOT EXISTS obligations(event TEXT PRIMARY KEY,revision TEXT,status TEXT);")?;
        db.execute(
            "INSERT OR IGNORE INTO state VALUES('inventory',?)",
            [inventory(&source)?],
        )?;
        db.execute("INSERT OR IGNORE INTO state VALUES('stopped','false')", [])?;
        Ok(Self {
            root: root.to_owned(),
            source,
            target,
            db,
            records: vec![],
        })
    }
    pub fn integrity(&self) -> Result<()> {
        let expected: String =
            self.db
                .query_row("SELECT value FROM state WHERE key='inventory'", [], |r| {
                    r.get(0)
                })?;
        let stopped: String =
            self.db
                .query_row("SELECT value FROM state WHERE key='stopped'", [], |r| {
                    r.get(0)
                })?;
        if stopped != "false" || !inventory(&self.source).is_ok_and(|value| value == expected) {
            self.db
                .execute("UPDATE state SET value='true' WHERE key='stopped'", [])?;
            return Err(
                "unknown source change: mutation/publication stopped; files preserved".into(),
            );
        }
        Ok(())
    }
    pub fn event(&self, event: &str, instruction: &str) -> Result<Notice> {
        let revision = file_hash(&self.target)?;
        self.db.execute(
            "INSERT INTO obligations VALUES(?,?,'pending')",
            params![event, revision],
        )?;
        Ok(Notice {
            event: event.into(),
            context: json!({"revision":revision,"instruction":instruction}),
        })
    }
    pub fn execute(&mut self, envelope: &Value) -> Result<EngineOutcome> {
        self.integrity()?;
        let request = &envelope["request"];
        let revision = file_hash(&self.target)?;
        let rows = {
            let mut statement = self.db.prepare(
                "SELECT event,revision,status FROM obligations WHERE status!='reconsidered'",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut accepted = false;
        let mut reason = "pending reconsideration or stale revision";
        let mut notice = None;
        match envelope["operation"].as_str() {
            Some("edit") => {
                accepted = rows.is_empty()
                    && *request
                        == json!({"request_id":"edit-1","expected_hash":revision,"content":"engine_updated\n"})
                    && fs::read_to_string(&self.target)? == "original\n";
                if accepted {
                    let temporary = self.root.join("controller/edit.tmp");
                    fs::write(&temporary, b"engine_updated\n")?;
                    fs::rename(&temporary, &self.target)?;
                    self.db.execute(
                        "UPDATE state SET value=? WHERE key='inventory'",
                        [inventory(&self.source)?],
                    )?;
                    reason = "controlled fixture edit";
                    notice=Some(self.event("E1",&format!("At the next tool boundary call falinks_review with action defer, event E1 and reviewed_hash {}; then call falinks_offer with expected_hash of that revision (expect rejection), then finish. Do not reconsider E1 yet.",file_hash(&self.target)?))?);
                }
            }
            Some("review") => {
                if let Some((event, expected, _)) = rows
                    .iter()
                    .find(|(event, _, _)| request["event"].as_str() == Some(event))
                {
                    accepted = request["reviewed_hash"] == *expected
                        && *expected == revision
                        && ["defer", "keep", "revise", "drop"]
                            .contains(&request["action"].as_str().unwrap_or(""));
                    if accepted {
                        reason = if request["action"] == "defer" {
                            "deferred"
                        } else {
                            "reconsidered"
                        };
                        self.db.execute(
                            "UPDATE obligations SET status=? WHERE event=?",
                            params![reason, event],
                        )?;
                    }
                }
            }
            Some("offer") => {
                accepted = rows.is_empty() && *request == json!({"expected_hash":revision});
                if accepted {
                    reason = "eligible controlled fixture; not publication";
                }
            }
            _ => {}
        }
        let result = json!({"accepted":accepted,"reason":reason});
        self.records
            .push(json!({"envelope":envelope,"result":result}));
        Ok(EngineOutcome { result, notice })
    }
}
