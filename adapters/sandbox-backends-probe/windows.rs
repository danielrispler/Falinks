//! Windows host: AppContainer restricts files and network, a Job Object owns the
//! process tree. Any setup failure returns Err before CreateProcessW: no plain fallback.
use crate::{Fixture, Launch, Loopback, Result, Scenario, wait_until};
use serde_json::{Value, json};
use std::{
    env,
    ffi::{OsStr, c_void},
    fs,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::Path,
    process::{self, Command},
    ptr::{null, null_mut},
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, HANDLE, HANDLE_FLAG_INHERIT, LocalFree, SetHandleInformation,
        WAIT_OBJECT_0,
    },
    Security::{
        ACL,
        Authorization::{
            ConvertSidToStringSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW,
            NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW,
            TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
        },
        DACL_SECURITY_INFORMATION, FreeSid,
        Isolation::{CreateAppContainerProfile, DeleteAppContainerProfile},
        PSECURITY_DESCRIPTOR, PSID, SECURITY_CAPABILITIES, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    },
    System::{
        JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        },
        Threading::{
            CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
            InitializeProcThreadAttributeList, OpenProcess,
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION, PROCESS_TERMINATE,
            STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
            WaitForSingleObject,
        },
    },
};

/// File rights spelled out: SetEntriesInAclW stores generic bits unmapped.
const FILE_GENERIC_READ: u32 = 0x0012_0089;
const FILE_GENERIC_EXECUTE: u32 = 0x0012_00A0;
const FILE_ALL_ACCESS: u32 = 0x001F_01FF;
/// PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT: makes the container an LPAC.
const ALL_APPLICATION_PACKAGES_OPT_OUT: u32 = 1;

pub fn facts() -> Value {
    let run = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .unwrap_or_else(|error| format!("error: {error}"))
    };
    json!({
        "ver": run("cmd", &["/c", "ver"]),
        "user": run("whoami", &[]),
        // S-1-16-12288 is the High mandatory level: an elevated token.
        "elevated": run("whoami", &["/groups"]).contains("S-1-16-12288"),
    })
}

pub fn plan() -> Vec<(&'static str, Scenario)> {
    vec![
        ("none", Scenario::Normal),
        ("job", Scenario::Normal),
        ("job", Scenario::Hang),
        ("appcontainer", Scenario::Normal),
        ("appcontainer", Scenario::Hang),
        ("lpac", Scenario::Normal),
        ("appcontainer", Scenario::Unavailable),
    ]
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

fn last_error(what: &str) -> Box<dyn std::error::Error> {
    // SAFETY: GetLastError reads thread-local state only.
    format!("{what} failed: error {}", unsafe { GetLastError() }).into()
}

fn check(ok: i32, what: &str) -> Result<()> {
    if ok == 0 {
        Err(last_error(what))
    } else {
        Ok(())
    }
}

fn win32(code: u32, what: &str) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("{what} failed: error {code}").into())
    }
}

/// One profile per fixture, so concurrent or leftover runs never share a container.
fn profile_name(fixture: &Fixture) -> String {
    let leaf = fixture
        .root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    format!("falinks.probe.{}.{leaf}", process::id())
}

/// Standard Windows argv quoting (backslashes double only before a quote).
fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.into();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                slashes = 0;
            }
            _ => slashes = 0,
        }
        if c != '\\' {
            out.push(c);
        } else {
            out.push('\\');
        }
    }
    out.extend(std::iter::repeat_n('\\', slashes));
    out.push('"');
    out
}

/// Grants `sid` `access` on `path`, inherited by everything beneath it.
fn grant(path: &Path, sid: PSID, access: u32) -> Result<()> {
    let name = wide(path.as_os_str());
    let mut old: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: name is NUL-terminated; out pointers are valid locals.
    let code = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old,
            null_mut(),
            &mut descriptor,
        )
    };
    win32(code, "GetNamedSecurityInfoW")?;
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: access,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
            ptstrName: sid.cast(),
        },
    };
    let mut new: *mut ACL = null_mut();
    // SAFETY: entry and old (owned by descriptor) outlive the call; new is a valid out pointer.
    let mut result = win32(
        unsafe { SetEntriesInAclW(1, &entry, old, &mut new) },
        "SetEntriesInAclW",
    );
    if result.is_ok() {
        // SAFETY: name is NUL-terminated and new is the ACL SetEntriesInAclW built.
        let code = unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                new,
                null(),
            )
        };
        result = win32(code, "SetNamedSecurityInfoW");
    }
    // SAFETY: both were LocalAlloc'd by the calls above (null is accepted).
    unsafe {
        LocalFree(new.cast());
        LocalFree(descriptor.cast());
    }
    result
}

/// Owns a container SID from CreateAppContainerProfile.
struct Container(PSID);

impl Drop for Container {
    fn drop(&mut self) {
        // SAFETY: the SID came from CreateAppContainerProfile and is freed once.
        unsafe { FreeSid(self.0) };
    }
}

fn container(name: &str) -> Result<Container> {
    let name = wide(OsStr::new(name));
    // Leftover profiles from an earlier crashed run are removed first.
    // SAFETY: name is NUL-terminated.
    unsafe { DeleteAppContainerProfile(name.as_ptr()) };
    let mut sid: PSID = null_mut();
    // SAFETY: strings are NUL-terminated; zero capabilities; sid is a valid out pointer.
    let hr = unsafe {
        CreateAppContainerProfile(
            name.as_ptr(),
            name.as_ptr(),
            name.as_ptr(),
            null(),
            0,
            &mut sid,
        )
    };
    if hr < 0 {
        return Err(format!("CreateAppContainerProfile failed: HRESULT {hr:#010x}").into());
    }
    Ok(Container(sid))
}

fn sid_string(sid: PSID) -> String {
    let mut text: *mut u16 = null_mut();
    // SAFETY: sid is a valid SID; text receives a LocalAlloc'd string freed below.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return "unknown".into();
    }
    // SAFETY: text is NUL-terminated per ConvertSidToStringSidW.
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    // SAFETY: len u16s were just read from text.
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    // SAFETY: allocated by ConvertSidToStringSidW.
    unsafe { LocalFree(text.cast()) };
    value
}

fn job() -> Result<HANDLE> {
    // SAFETY: anonymous job, default security.
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job.is_null() {
        return Err(last_error("CreateJobObjectW"));
    }
    // SAFETY: zeroed is a valid all-defaults limit structure.
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    // No JOB_OBJECT_LIMIT_BREAKAWAY_OK: descendants cannot leave the job.
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: limits is a correctly sized, initialized structure for this class.
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        let error = last_error("SetInformationJobObject");
        // SAFETY: job is a handle this function owns.
        unsafe { CloseHandle(job) };
        return Err(error);
    }
    Ok(job)
}

fn active_processes(job: HANDLE) -> Result<u32> {
    // SAFETY: zeroed is valid for this plain-data structure.
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: info is a correctly sized out buffer for this class.
    let ok = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&raw mut info).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    };
    check(ok, "QueryInformationJobObject")?;
    Ok(info.ActiveProcesses)
}

fn inheritable(file: &fs::File) -> Result<HANDLE> {
    let handle = file.as_raw_handle() as HANDLE;
    check(
        // SAFETY: handle is owned by file, which outlives process creation.
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) },
        "SetHandleInformation",
    )?;
    Ok(handle)
}

pub fn launch(
    mechanism: &str,
    scenario: Scenario,
    fixture: &Fixture,
    loopback: &Loopback,
    timeout: Duration,
) -> Result<Launch> {
    let contained = matches!(mechanism, "appcontainer" | "lpac");
    let jobbed = mechanism != "none";
    // The child runs from a private copy so the container is granted only that directory.
    let bin = fixture.root.join("bin");
    fs::create_dir_all(&bin)?;
    let exe = bin.join("probe.exe");
    fs::copy(env::current_exe()?, &exe)?;
    let mut extra = json!({});
    let sandbox = if contained {
        let name = if scenario == Scenario::Unavailable {
            // Over the 64-character limit: the production setup path must fail closed.
            "x".repeat(65)
        } else {
            profile_name(fixture)
        };
        let sandbox = container(&name)?;
        extra["container_sid"] = json!(sid_string(sandbox.0));
        grant(&bin, sandbox.0, FILE_GENERIC_READ | FILE_GENERIC_EXECUTE)?;
        grant(&fixture.input, sandbox.0, FILE_GENERIC_READ)?;
        grant(&fixture.output, sandbox.0, FILE_ALL_ACCESS)?;
        Some(sandbox)
    } else {
        None
    };
    let job = if jobbed { Some(job()?) } else { None };
    let result = spawn(
        mechanism,
        &exe,
        fixture,
        loopback,
        scenario,
        sandbox.as_ref(),
        job,
        timeout,
        &mut extra,
    );
    if let Some(job) = job {
        // SAFETY: job is owned here; KILL_ON_JOB_CLOSE ends anything still inside.
        unsafe { CloseHandle(job) };
    }
    let mut launch = result?;
    launch.extra = extra;
    Ok(launch)
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    mechanism: &str,
    exe: &Path,
    fixture: &Fixture,
    loopback: &Loopback,
    scenario: Scenario,
    sandbox: Option<&Container>,
    job: Option<HANDLE>,
    timeout: Duration,
    extra: &mut Value,
) -> Result<Launch> {
    let stdout = fs::File::create(fixture.evidence.join("stdout"))?;
    let stderr = fs::File::create(fixture.evidence.join("stderr"))?;
    let stdin = fs::File::open("NUL")?;
    let handles = [
        inheritable(&stdin)?,
        inheritable(&stdout)?,
        inheritable(&stderr)?,
    ];
    let mut line = quote(&exe.display().to_string());
    for arg in crate::child_args(fixture, loopback, scenario) {
        line.push(' ');
        line.push_str(&quote(&arg));
    }
    let mut line = wide(OsStr::new(&line));
    let output = fixture.output.display().to_string();
    let root = env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let mut block = String::new();
    for (key, value) in [
        ("PATH", format!(r"{root}\System32")),
        ("SystemRoot", root.clone()),
        ("TEMP", output.clone()),
        ("TMP", output.clone()),
        ("USERPROFILE", output.clone()),
    ] {
        block.push_str(&format!("{key}={value}\0"));
    }
    block.push('\0');
    let block = block.encode_utf16().collect::<Vec<_>>();
    let cwd = wide(fixture.output.as_os_str());

    // Attribute values must stay alive until CreateProcessW returns.
    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: sandbox.map_or(null_mut(), |s| s.0),
        Capabilities: null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let jobs = job.map(|j| [j]);
    let policy = ALL_APPLICATION_PACKAGES_OPT_OUT;
    let mut attributes: Vec<(u32, *const c_void, usize)> = vec![(
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        handles.as_ptr().cast(),
        size_of_val(&handles),
    )];
    if let Some(jobs) = &jobs {
        attributes.push((
            PROC_THREAD_ATTRIBUTE_JOB_LIST,
            jobs.as_ptr().cast(),
            size_of_val(jobs),
        ));
    }
    if sandbox.is_some() {
        attributes.push((
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            (&raw const capabilities).cast(),
            size_of::<SECURITY_CAPABILITIES>(),
        ));
    }
    if mechanism == "lpac" {
        attributes.push((
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            (&raw const policy).cast(),
            size_of::<u32>(),
        ));
    }
    let mut size = 0;
    // SAFETY: a null list asks only for the required size; this call reports failure by design.
    unsafe { InitializeProcThreadAttributeList(null_mut(), attributes.len() as u32, 0, &mut size) };
    let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
    let list = storage.as_mut_ptr().cast();
    check(
        // SAFETY: storage is at least `size` bytes and pointer-aligned.
        unsafe { InitializeProcThreadAttributeList(list, attributes.len() as u32, 0, &mut size) },
        "InitializeProcThreadAttributeList",
    )?;
    let created = (|| {
        for &(attribute, value, len) in &attributes {
            check(
                // SAFETY: list is initialized; value points at `len` live bytes for the right type.
                unsafe {
                    UpdateProcThreadAttribute(
                        list,
                        0,
                        attribute as usize,
                        value,
                        len,
                        null_mut(),
                        null(),
                    )
                },
                "UpdateProcThreadAttribute",
            )?;
        }
        // SAFETY: zeroed is valid for STARTUPINFOEXW; required fields are set below.
        let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = list;
        // SAFETY: zeroed is valid for this out structure.
        let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: every buffer is NUL-terminated and alive; the handle list limits inheritance.
        let ok = unsafe {
            CreateProcessW(
                null(),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                block.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut info,
            )
        };
        check(ok, "CreateProcessW").map(|_| info)
    })();
    // SAFETY: list was initialized above and is no longer used by the OS.
    unsafe { DeleteProcThreadAttributeList(list) };
    let info = created?;
    // SAFETY: the thread handle is unused.
    unsafe { CloseHandle(info.hThread) };
    let process = info.hProcess;
    let (exit, timed_out) = wait_until(timeout, || {
        // SAFETY: process is a live handle owned here.
        if unsafe { WaitForSingleObject(process, 0) } != WAIT_OBJECT_0 {
            return Ok(None);
        }
        let mut code = 0;
        check(
            // SAFETY: process has exited; code is a valid out pointer.
            unsafe { GetExitCodeProcess(process, &mut code) },
            "GetExitCodeProcess",
        )?;
        Ok(Some(format!("exit code {code}")))
    })?;
    let mut tree_survivors = None;
    if let Some(job) = job {
        if !timed_out {
            let active = active_processes(job)?;
            extra["active_processes_after_exit"] = json!(active);
            tree_survivors = Some(active > 0);
        }
        // Timeout or survivors: the whole job is killed, mirroring the engine's group kill.
        // SAFETY: job is a live handle owned by the caller.
        unsafe { TerminateJobObject(job, 1) };
        thread::sleep(Duration::from_millis(200));
        let after = active_processes(job)?;
        extra["active_processes_after_kill"] = json!(after);
        if timed_out {
            tree_survivors = Some(after > 0);
        }
    } else if timed_out {
        // SAFETY: process is a live handle owned here.
        unsafe { TerminateProcess(process, 1) };
    }
    // SAFETY: process is owned here and closed once.
    unsafe { CloseHandle(process) };
    Ok(Launch {
        started: true,
        launch_error: None,
        exit,
        timed_out,
        tree_survivors,
        extra: Value::Null,
    })
}

pub fn cleanup(fixture: &Fixture) {
    let name = wide(OsStr::new(&profile_name(fixture)));
    // SAFETY: name is NUL-terminated; a missing profile is ignored.
    unsafe { DeleteAppContainerProfile(name.as_ptr()) };
    let text = fs::read_to_string(fixture.output.join("child.json")).unwrap_or_default();
    let pid = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|child| child["descendant"].as_str().map(String::from))
        .and_then(|d| d.split("pid ").nth(1).and_then(|p| p.parse::<u32>().ok()));
    if let Some(pid) = pid {
        // SAFETY: best-effort kill of the probe's own detached descendant.
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !process.is_null() {
                TerminateProcess(process, 1);
                CloseHandle(process);
            }
        }
    }
}
