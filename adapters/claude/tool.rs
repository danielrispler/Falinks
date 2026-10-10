//! Host-launched helpers run by Claude Code outside the worker sandbox: the `falinks`
//! stdio MCP server and the `PreToolUse` identity hook. Both forward to the trusted host
//! over its controller socket; neither touches the workspace itself.
use falinks_host::{Result, require, runtime::tools};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    process,
};

fn host(controller: &Path, mut request: Value) -> Result<Value> {
    request["token"] = json!(fs::read_to_string(controller.join("token"))?);
    let mut stream = UnixStream::connect(controller.join("host.sock"))?;
    writeln!(stream, "{request}")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}

/// Register the runtime's own identity stamp for a falinks call before it runs.
fn hook(controller: &Path) -> Result<()> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    let input: Value = serde_json::from_str(&raw)?;
    let reply = host(
        controller,
        json!({"kind":"hook","session":input["session_id"],"prompt_id":input["prompt_id"],
            "tool_use_id":input["tool_use_id"],"tool_name":input["tool_name"],"tool_input":input["tool_input"]}),
    )?;
    require(
        reply["allow"] == true,
        reply["reason"].as_str().unwrap_or("host refused the call"),
    )
}

fn mcp(controller: &Path, workspace: &str) -> Result<()> {
    let session = env::var("CLAUDE_CODE_SESSION_ID").unwrap_or_default();
    let mut out = std::io::stdout();
    for line in BufReader::new(std::io::stdin()).lines() {
        let message: Value = serde_json::from_str(&line?)?;
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let params = &message["params"];
        let reply = match message["method"].as_str().unwrap_or("") {
            "initialize" => {
                json!({"result":{"protocolVersion":params["protocolVersion"],"capabilities":{"tools":{}},
                "serverInfo":{"name":"falinks","version":"1"}}})
            }
            "tools/list" => json!({"result":{"tools":tools(workspace)}}),
            "tools/call" => {
                let result = host(
                    controller,
                    json!({"kind":"call","session":session,"tool_use_id":params["_meta"]["claudecode/toolUseId"],
                        "tool":params["name"],"arguments":params["arguments"]}),
                )
                .unwrap_or_else(|error| json!({"accepted":false,"reason":error.to_string()}));
                json!({"result":{"content":[{"type":"text","text":result.to_string()}],
                    "isError":result["accepted"] != true}})
            }
            _ => json!({"error":{"code":-32601,"message":"Host capability disabled"}}),
        };
        let mut reply = reply;
        reply["jsonrpc"] = json!("2.0");
        reply["id"] = id;
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    let result = match (args.get(1).map(String::as_str), args.len()) {
        (Some("hook"), 3) => hook(Path::new(&args[2])),
        (Some("mcp"), 4) => mcp(Path::new(&args[2]), &args[3]),
        _ => Err("usage: falinks-claude-tool hook CONTROLLER | mcp CONTROLLER WORKSPACE".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        // Exit code 2 blocks the tool call when this runs as a hook.
        process::exit(2);
    }
}
