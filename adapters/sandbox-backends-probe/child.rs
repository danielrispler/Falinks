//! The contained program: tries each contract violation and records what happened.
//! Every outcome is kept as text so evidence shows the exact error, not a verdict.
use crate::{CANARY, Result};
use serde_json::{Map, Value, json};
use std::{
    env, fs,
    io::Write,
    net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn outcome<T>(result: std::io::Result<T>) -> Value {
    match result {
        Ok(_) => json!("ok"),
        Err(error) => json!(format!("error: {error}")),
    }
}

/// Minimal DNS query for example.com A, sent raw so UDP is tested without the resolver.
const DNS_QUERY: &[u8] = &[
    0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c',
    b'o', b'm', 0, 0, 1, 0, 1,
];

fn udp() -> Value {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => socket,
        Err(error) => return json!(format!("bind error: {error}")),
    };
    if let Err(error) = socket.send_to(DNS_QUERY, "8.8.8.8:53") {
        return json!(format!("send error: {error}"));
    }
    let _ = socket.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buffer = [0; 512];
    match socket.recv_from(&mut buffer) {
        Ok((len, _)) => json!(format!("reply {len} bytes")),
        Err(error) => json!(format!("recv error: {error}")),
    }
}

/// AppContainer denies opening `NUL`; inherit the host-provided handles there instead.
fn null_or_inherit() -> Stdio {
    if cfg!(windows) {
        Stdio::inherit()
    } else {
        Stdio::null()
    }
}

/// Starts a descendant that tries to leave the assigned process tree.
fn spawn_descendant(output: &Path) -> Value {
    let mut command = Command::new(match env::current_exe() {
        Ok(exe) => exe,
        Err(error) => return json!(format!("current_exe error: {error}")),
    });
    command
        .arg("descendant")
        .arg(output)
        .stdin(null_or_inherit())
        .stdout(null_or_inherit())
        .stderr(null_or_inherit());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and allocates nothing; it runs before exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x8;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        command.creation_flags(
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB,
        );
        match command.spawn() {
            Ok(child) => return json!(format!("breakaway spawned pid {}", child.id())),
            Err(error) => {
                // Breakaway is refused inside a job without BREAKAWAY_OK; still try to detach.
                command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
                return match command.spawn() {
                    Ok(child) => json!(format!(
                        "breakaway refused ({error}); detached pid {}",
                        child.id()
                    )),
                    Err(second) => {
                        json!(format!("breakaway error: {error}; detach error: {second}"))
                    }
                };
            }
        }
    }
    #[allow(unreachable_code)]
    match command.spawn() {
        Ok(child) => json!(format!("setsid pid {}", child.id())),
        Err(error) => json!(format!("spawn error: {error}")),
    }
}

/// Detached descendant: proves liveness through `output/heartbeat`, then exits on its own.
pub fn descendant(args: &[String]) -> Result<()> {
    let output = Path::new(args.first().ok_or("missing output")?);
    let started = Instant::now();
    let mut beat = 0u64;
    while started.elapsed() < Duration::from_secs(60) {
        beat += 1;
        fs::write(output.join("heartbeat"), beat.to_string())?;
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    let root = Path::new(args.first().ok_or("missing root")?);
    let port: u16 = args.get(1).ok_or("missing port")?.parse()?;
    let home = Path::new(args.get(2).ok_or("missing home")?);
    let hang = args.get(3).is_some_and(|x| x == "hang");
    let (input, output, secret) = (root.join("input"), root.join("output"), root.join("secret"));
    let mut checks = Map::new();
    let mut names = env::vars_os()
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    checks.insert("env_names".into(), json!(names));
    checks.insert(
        "env_canary_present".into(),
        json!(env::var_os(CANARY).is_some()),
    );
    checks.insert("read_input".into(), outcome(fs::read(input.join("in.txt"))));
    checks.insert(
        "read_secret".into(),
        outcome(fs::read(secret.join("secret.txt"))),
    );
    checks.insert("list_root_fixture".into(), outcome(fs::read_dir(root)));
    checks.insert("list_user_home".into(), outcome(fs::read_dir(home)));
    checks.insert(
        "write_output".into(),
        outcome(fs::write(output.join("o.txt"), b"o")),
    );
    checks.insert(
        "write_input".into(),
        outcome(fs::write(input.join("in.txt"), b"X")),
    );
    checks.insert(
        "write_secret".into(),
        outcome(fs::write(secret.join("new.txt"), b"X")),
    );
    // A fixed system temp path: TMPDIR points into output, so temp_dir() proves nothing.
    let temp = if cfg!(windows) {
        r"C:\Users\Public"
    } else {
        "/tmp"
    };
    checks.insert(
        "write_system_temp".into(),
        outcome(fs::write(
            Path::new(temp).join("falinks-probe-escape"),
            b"X",
        )),
    );
    checks.insert(
        "rename_input_into_output".into(),
        outcome(fs::rename(input.join("in.txt"), output.join("moved.txt"))),
    );
    checks.insert(
        "open_null_device".into(),
        outcome(fs::File::open(if cfg!(windows) {
            "NUL"
        } else {
            "/dev/null"
        })),
    );
    checks.insert("descendant".into(), spawn_descendant(&output));
    // Network last, isolated: LPAC fails WSAStartup and Rust std panics on first socket use.
    let mut network_checks = Map::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        network(&mut network_checks, port)
    }));
    if let Err(panic) = result {
        let text = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        network_checks.insert("network_panic".into(), json!(text));
    }
    checks.extend(network_checks);
    let report = Value::Object(checks);
    // Evidence goes to output, the one writable place; stdout is the backup.
    fs::File::create(output.join("child.json"))?.write_all(report.to_string().as_bytes())?;
    println!("PROBE_CHILD={report}");
    if hang {
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }
    Ok(())
}

fn network(checks: &mut Map<String, Value>, port: u16) {
    let internet: SocketAddr = "1.1.1.1:443".parse().expect("address");
    checks.insert(
        "tcp_internet".into(),
        outcome(TcpStream::connect_timeout(
            &internet,
            Duration::from_secs(3),
        )),
    );
    let loopback: SocketAddr = format!("127.0.0.1:{port}").parse().expect("address");
    checks.insert(
        "tcp_loopback".into(),
        outcome(TcpStream::connect_timeout(
            &loopback,
            Duration::from_secs(3),
        )),
    );
    checks.insert(
        "tcp_bind".into(),
        outcome(std::net::TcpListener::bind("0.0.0.0:0")),
    );
    checks.insert("udp_dns_8888".into(), udp());
    checks.insert(
        "dns_resolve".into(),
        match ("example.com", 443).to_socket_addrs() {
            Ok(addrs) => json!(format!("resolved {} addresses", addrs.count())),
            Err(error) => json!(format!("error: {error}")),
        },
    );
    #[cfg(target_os = "linux")]
    {
        use std::os::{linux::net::SocketAddrExt, unix::net};
        // Host daemons reachable over pathname sockets bypass "no network" indirectly.
        for path in [
            "/run/systemd/resolve/io.systemd.Resolve",
            "/run/dbus/system_bus_socket",
            "/run/systemd/journal/stdout",
        ] {
            if Path::new(path).exists() {
                checks.insert(
                    format!("unix_connect {path}"),
                    outcome(net::UnixStream::connect(path)),
                );
            }
        }
        let name = format!("falinks-probe-{port}");
        checks.insert(
            "unix_abstract_connect_host".into(),
            outcome(
                net::SocketAddr::from_abstract_name(name.as_bytes())
                    .and_then(|addr| net::UnixStream::connect_addr(&addr)),
            ),
        );
    }
}
