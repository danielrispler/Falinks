//! Scaffolding shared by the check binaries: their common options and how a report is
//! finished. Each binary keeps its own scenario, outcome and exit code.
use crate::{Result, require};
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Options {
    pub binary: PathBuf,
    pub output: PathBuf,
    pub probe: PathBuf,
    pub tool: PathBuf,
}
impl Options {
    /// Parses `--binary`, `--output`, `--probe` and `--tool` pairs. `--probe` and `--tool`
    /// default to the `sandbox-probe` and `tool_name` executables beside this one. `--help`
    /// prints `usage` and exits 0; any other error is printed and exits 1.
    pub fn parse(args: &[String], tool_name: &str, usage: &str) -> Self {
        if args == ["--help"] {
            println!("{usage}");
            process::exit(0);
        }
        Self::try_parse(args, tool_name).unwrap_or_else(|error| {
            eprintln!("{error}");
            process::exit(1);
        })
    }
    fn try_parse(args: &[String], tool_name: &str) -> Result<Self> {
        require(args.len().is_multiple_of(2), "expected option/value pairs")?;
        let (mut binary, mut output, mut probe, mut tool) = (None, None, None, None);
        for pair in args.as_chunks::<2>().0 {
            let value = Some(PathBuf::from(&pair[1]));
            match pair[0].as_str() {
                "--binary" => binary = value,
                "--output" => output = value,
                "--probe" => probe = value,
                "--tool" => tool = value,
                _ => return Err("unknown option".into()),
            }
        }
        let sibling = |name| env::current_exe().map(|exe| exe.with_file_name(name));
        let probe = probe.map_or_else(|| sibling("sandbox-probe"), Ok)?;
        let tool = tool.map_or_else(|| sibling(tool_name), Ok)?;
        // Name a missing worker tool before any runtime launch, not a bare os error later.
        for path in [&probe, &tool] {
            require(path.is_file(), &format!("missing {}", path.display()))?;
        }
        Ok(Self {
            binary: binary.ok_or("--binary required")?,
            output: output.ok_or("--output required")?,
            probe,
            tool,
        })
    }
}

/// The fields every report starts with. A report never enables publication or scoring.
pub fn report(runtime: &str) -> Value {
    json!({"implementation":"rust","runtime":runtime,"publication_scoring_supported":false,
        "started_at_unix_seconds":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()})
}

/// Writes `report` to `output` (creating its folder) and prints `summary` as one line.
/// A report that cannot be written exits 1.
pub fn finish(output: &Path, report: &Value, summary: Value) {
    let written = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(output, serde_json::to_string_pretty(report).unwrap() + "\n"));
    if let Err(error) = written {
        eprintln!("{error}");
        process::exit(1);
    }
    println!("{summary}");
}
