use super::*;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::WinTrust::*;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Threading::*;

struct Handle(HANDLE, Arc<Entry>);
unsafe impl Send for Handle {}

impl Handle {
    fn new(handle: HANDLE, entry: &Arc<Entry>) -> Result<Self, Failure> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(Failure::Process)
        } else {
            entry.open_handles.fetch_add(1, Ordering::AcqRel);
            entry
                .observation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .handles_closed = false;
            Ok(Self(handle, entry.clone()))
        }
    }
    fn close(&mut self) -> Result<(), Failure> {
        if self.0.is_null() {
            return Ok(());
        }
        if unsafe { CloseHandle(self.0) } == 0 {
            return Err(Failure::Cleanup);
        }
        self.0 = null_mut();
        if self.1.open_handles.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.1
                .observation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .handles_closed = true;
        }
        Ok(())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        while self.close().is_err() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn wide(value: &std::ffi::OsStr) -> Result<Vec<u16>, Failure> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(Failure::UnsafePath);
    }
    value.push(0);
    Ok(value)
}

fn child_path(path: &Path) -> PathBuf {
    let value: Vec<_> = path.as_os_str().encode_wide().collect();
    if value.starts_with(&[92, 92, 63, 92]) {
        PathBuf::from(std::ffi::OsString::from_wide(&value[4..]))
    } else {
        path.to_path_buf()
    }
}

fn checked(path: &Path, directory: bool) -> Result<(PathBuf, File), Failure> {
    if !path.is_absolute() {
        return Err(Failure::UnsafePath);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        use std::path::{Component, Prefix};
        match component {
            Component::ParentDir | Component::CurDir => return Err(Failure::UnsafePath),
            Component::Prefix(prefix)
                if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) =>
            {
                return Err(Failure::UnsafePath)
            }
            Component::Normal(name)
                if name.to_string_lossy().contains(':')
                    || name.to_string_lossy().ends_with(['.', ' ']) =>
            {
                return Err(Failure::UnsafePath)
            }
            _ => current.push(component),
        }
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&current).map_err(|_| Failure::UnsafePath)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Failure::UnsafePath);
        }
    }
    let path = path.canonicalize().map_err(|_| Failure::UnsafePath)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(if directory { 3 } else { 1 })
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        )
        .open(&path)
        .map_err(|_| Failure::UnsafePath)?;
    let metadata = file.metadata().map_err(|_| Failure::UnsafePath)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.is_dir() != directory
    {
        return Err(Failure::UnsafePath);
    }
    Ok((path, file))
}

fn digest(file: &mut File) -> Result<[u8; 32], Failure> {
    file.rewind().map_err(|_| Failure::Untrusted)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let read = file.read(&mut buffer).map_err(|_| Failure::Untrusted)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(hash.finalize().into())
}

pub(super) fn trusted(path: &Path, expected: [u8; 32]) -> Result<TrustedFile, Failure> {
    let (path, mut file) = checked(path, false)?;
    if digest(&mut file)? != expected {
        return Err(Failure::Untrusted);
    }
    Ok(TrustedFile {
        path,
        sha256: expected,
        _pin: file,
    })
}

pub(super) fn signed(path: &Path) -> Result<TrustedFile, Failure> {
    let (path, mut file) = checked(path, false)?;
    let path_wide = wide(path.as_os_str())?;
    let mut info: WINTRUST_FILE_INFO = unsafe { zeroed() };
    info.cbStruct = size_of::<WINTRUST_FILE_INFO>() as u32;
    info.pcwszFilePath = path_wide.as_ptr();
    let mut data: WINTRUST_DATA = unsafe { zeroed() };
    data.cbStruct = size_of::<WINTRUST_DATA>() as u32;
    data.dwUIChoice = WTD_UI_NONE;
    data.fdwRevocationChecks = WTD_REVOKE_NONE;
    data.dwUnionChoice = WTD_CHOICE_FILE;
    data.Anonymous.pFile = &mut info;
    data.dwStateAction = WTD_STATEACTION_VERIFY;
    data.dwProvFlags =
        WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE | WTD_DISABLE_MD2_MD4;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let result = unsafe {
        WinVerifyTrust(
            INVALID_HANDLE_VALUE,
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    let closed = unsafe {
        WinVerifyTrust(
            INVALID_HANDLE_VALUE,
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    if result != 0 || closed != 0 {
        return Err(Failure::Untrusted);
    }
    let sha256 = digest(&mut file)?;
    Ok(TrustedFile {
        path,
        sha256,
        _pin: file,
    })
}

pub(super) fn snapshot(root: &Path) -> Result<(PathBuf, File), Failure> {
    let (root, pin) = checked(root, true)?;
    if root
        .file_name()
        .is_none_or(|name| !name.to_string_lossy().starts_with("zabor-electron-copy-"))
    {
        return Err(Failure::UnsafePath);
    }
    let (_, mut marker) = checked(&root.join(".native-copy-snapshot"), false)?;
    let mut bytes = Vec::new();
    marker
        .by_ref()
        .take(32)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::UnsafePath)?;
    if bytes != b"zabor-electron-copy-v1" {
        return Err(Failure::UnsafePath);
    }
    Ok((root, pin))
}

fn environment(root: &Path) -> Result<Vec<u16>, Failure> {
    let mut values: BTreeMap<String, std::ffi::OsString> = BTreeMap::new();
    for (key, value) in std::env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if [
            "SYSTEMROOT",
            "WINDIR",
            "SYSTEMDRIVE",
            "PATH",
            "COMSPEC",
            "PATHEXT",
            "PROCESSOR_ARCHITECTURE",
            "NUMBER_OF_PROCESSORS",
            "USERNAME",
            "USERDOMAIN",
        ]
        .contains(&name.as_str())
        {
            values.insert(name, value);
        }
    }
    for name in [
        "APPDATA",
        "LOCALAPPDATA",
        "HOME",
        "USERPROFILE",
        "TEMP",
        "TMP",
    ] {
        values.insert(name.into(), root.as_os_str().into());
    }
    let mut output = Vec::new();
    for (key, value) in values {
        let key: Vec<_> = std::ffi::OsStr::new(&key).encode_wide().collect();
        let value: Vec<_> = value.encode_wide().collect();
        if key.contains(&0) || value.contains(&0) {
            return Err(Failure::InvalidRequest);
        }
        output.extend(key);
        output.push(b'=' as u16);
        output.extend(value);
        output.push(0);
    }
    output.push(0);
    Ok(output)
}

fn quote(path: &std::ffi::OsStr) -> Result<Vec<u16>, Failure> {
    let input: Vec<_> = path.encode_wide().collect();
    if input.contains(&0) {
        return Err(Failure::UnsafePath);
    }
    let mut output = vec![b'"' as u16];
    let mut slashes = 0;
    for character in input {
        if character == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if character == b'"' as u16 {
            output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
        } else {
            output.extend(std::iter::repeat_n(b'\\' as u16, slashes));
        }
        slashes = 0;
        output.push(character);
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    Ok(output)
}

struct Attributes {
    words: Vec<usize>,
    initialized: bool,
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.words.as_mut_ptr().cast());
            }
        }
    }
}

struct Child {
    process: Handle,
    thread: Handle,
    pid: u32,
}

type PipeResult = (Result<Zeroizing<Vec<u8>>, Failure>, bool, bool);

struct ReaderOwner {
    thread: Option<JoinHandle<PipeResult>>,
    stop: Arc<AtomicBool>,
    entry: Arc<Entry>,
}

impl Drop for ReaderOwner {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.stop.store(true, Ordering::Release);
            let result = thread.join();
            let mut observation = self
                .entry
                .observation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            observation.reader_joined = true;
            if let Ok((bytes, eof, _)) = result {
                drop(bytes);
                observation.pipe_eof = eof;
            }
        }
    }
}

struct ProcessOwner {
    job: Handle,
    child: Child,
    assigned: bool,
    retired: bool,
    entry: Arc<Entry>,
}

impl Drop for ProcessOwner {
    fn drop(&mut self) {
        if !self.retired {
            loop {
                unsafe {
                    if self.assigned {
                        TerminateJobObject(self.job.0, 1);
                    } else {
                        TerminateProcess(self.child.process.0, 1);
                    }
                }
                let reaped =
                    unsafe { WaitForSingleObject(self.child.process.0, 10) } == WAIT_OBJECT_0;
                if reaped && active(&self.job) == Ok(0) {
                    let mut observation = self
                        .entry
                        .observation
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    observation.root_reaped = true;
                    observation.job_empty = true;
                    observation.children_terminated = self.assigned;
                    let mut exit = 0;
                    if unsafe { GetExitCodeProcess(self.child.process.0, &mut exit) } != 0 {
                        observation.exit_code = Some(exit);
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn spawn(request: &ExportRequest, writer: &Handle, null_handle: &Handle) -> Result<Child, Failure> {
    let runtime = child_path(&request.runtime.path);
    let helper = child_path(&request.helper.path);
    let root = child_path(&request.root);
    let application = wide(runtime.as_os_str())?;
    let mut arguments = vec![
        runtime.as_os_str().to_os_string(),
        helper.as_os_str().to_os_string(),
        root.as_os_str().to_os_string(),
    ];
    if !request.fixture {
        arguments.extend(
            [
                format!("--user-data-dir={}", root.display()),
                "--disable-background-networking".into(),
                "--disable-component-update".into(),
                "--disable-domain-reliability".into(),
                "--no-proxy-server".into(),
                "--host-resolver-rules=MAP * ~NOTFOUND".into(),
            ]
            .into_iter()
            .map(Into::into),
        );
    }
    let mut command = Vec::new();
    for argument in arguments {
        if !command.is_empty() {
            command.push(b' ' as u16);
        }
        command.extend(quote(&argument)?);
    }
    command.push(0);
    let env = Zeroizing::new(environment(&root)?);
    let directory = wide(root.as_os_str())?;
    let mut bytes = 0;
    unsafe {
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
    }
    if bytes == 0 {
        return Err(Failure::Process);
    }
    let mut attributes = Attributes {
        words: vec![0; bytes.div_ceil(size_of::<usize>())],
        initialized: false,
    };
    if unsafe {
        InitializeProcThreadAttributeList(attributes.words.as_mut_ptr().cast(), 1, 0, &mut bytes)
    } == 0
    {
        return Err(Failure::Process);
    }
    attributes.initialized = true;
    let handles = [writer.0, null_handle.0];
    if unsafe {
        UpdateProcThreadAttribute(
            attributes.words.as_mut_ptr().cast(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            size_of_val(&handles),
            null_mut(),
            null(),
        )
    } == 0
    {
        return Err(Failure::Process);
    }
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdOutput = writer.0;
    startup.StartupInfo.hStdError = null_handle.0;
    startup.StartupInfo.hStdInput = null_handle.0;
    startup.lpAttributeList = attributes.words.as_mut_ptr().cast();
    let mut info: PROCESS_INFORMATION = unsafe { zeroed() };
    if unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT,
            env.as_ptr().cast(),
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        )
    } == 0
    {
        return Err(Failure::Process);
    }
    Ok(Child {
        process: Handle::new(info.hProcess, &writer.1)?,
        thread: Handle::new(info.hThread, &writer.1)?,
        pid: info.dwProcessId,
    })
}

fn reader(
    mut pipe: Handle,
    max: usize,
    stop: &AtomicBool,
    state: &AtomicU8,
) -> (Result<Zeroizing<Vec<u8>>, Failure>, bool, bool) {
    let mut output = Zeroizing::new(Vec::new());
    if output.try_reserve_exact(max).is_err() {
        state.store(4, Ordering::Release);
        return (Err(Failure::Reader), false, pipe.close().is_ok());
    }
    let mut buffer = Zeroizing::new([0_u8; 65536]);
    let result = loop {
        if stop.load(Ordering::Acquire) {
            state.store(3, Ordering::Release);
            break Err(Failure::Cancelled);
        }
        let mut available = 0;
        if unsafe {
            PeekNamedPipe(
                pipe.0,
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        } == 0
        {
            if unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
                state.store(1, Ordering::Release);
                break Ok(output);
            }
            state.store(4, Ordering::Release);
            break Err(Failure::Reader);
        }
        if available == 0 {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let mut read = 0;
        let count = (available as usize).min(buffer.len());
        if unsafe {
            ReadFile(
                pipe.0,
                buffer.as_mut_ptr().cast(),
                count as u32,
                &mut read,
                null_mut(),
            )
        } == 0
        {
            state.store(4, Ordering::Release);
            break Err(Failure::Reader);
        }
        if output.len().saturating_add(read as usize) > max {
            state.store(2, Ordering::Release);
            break Err(Failure::Oversize);
        }
        output.extend_from_slice(&buffer[..read as usize]);
    };
    let eof = state.load(Ordering::Acquire) == 1;
    let closed = pipe.close().is_ok();
    (result, eof, closed)
}

fn active(job: &Handle) -> Result<u32, Failure> {
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
    if unsafe {
        QueryInformationJobObject(
            job.0,
            JobObjectBasicAccountingInformation,
            (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of_val(&info) as u32,
            null_mut(),
        )
    } == 0
    {
        return Err(Failure::Cleanup);
    }
    Ok(info.ActiveProcesses)
}

pub(super) fn execute(request: ExportRequest, limits: Limits, entry: &Arc<Entry>) -> ActorResult {
    let mut observation = Observation {
        operation: entry.id,
        owner_started: true,
        ..Observation::default()
    };
    let result = execute_owned(&request, limits, entry, &mut observation);
    let retired = entry
        .observation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    observation.root_reaped |= retired.root_reaped;
    observation.exit_code = observation.exit_code.or(retired.exit_code);
    observation.job_empty |= retired.job_empty;
    observation.reader_joined |= retired.reader_joined;
    observation.pipe_eof |= retired.pipe_eof;
    observation.children_terminated |= retired.children_terminated;
    observation.handles_closed = entry.open_handles.load(Ordering::Acquire) == 0;
    observation.failure = result.as_ref().err().copied();
    ActorResult {
        bytes: result,
        observation,
    }
}

fn execute_owned(
    request: &ExportRequest,
    limits: Limits,
    entry: &Arc<Entry>,
    observation: &mut Observation,
) -> Result<Zeroizing<Vec<u8>>, Failure> {
    if entry.cancelled.load(Ordering::Acquire) {
        return Err(Failure::Cancelled);
    }
    let job = Handle::new(unsafe { CreateJobObjectW(null(), null()) }, entry)?;
    observation.job_created = true;
    let mut limit: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limit.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limit as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of_val(&limit) as u32,
        )
    } == 0
    {
        return Err(Failure::Process);
    }
    let mut security: SECURITY_ATTRIBUTES = unsafe { zeroed() };
    security.nLength = size_of::<SECURITY_ATTRIBUTES>() as u32;
    security.bInheritHandle = 1;
    let mut input = null_mut();
    let mut output = null_mut();
    if unsafe { CreatePipe(&mut input, &mut output, &security, 65536) } == 0 {
        return Err(Failure::Process);
    }
    let input = Handle::new(input, entry)?;
    let mut output = Handle::new(output, entry)?;
    if unsafe { SetHandleInformation(input.0, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(Failure::Process);
    }
    let null_name = wide(std::ffi::OsStr::new("NUL"))?;
    let mut sink = Handle::new(
        unsafe {
            CreateFileW(
                null_name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &security,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        },
        entry,
    )?;
    if entry.cancelled.load(Ordering::Acquire) {
        return Err(Failure::Cancelled);
    }
    let child = spawn(request, &output, &sink)?;
    observation.pid = Some(child.pid);
    let mut owned = ProcessOwner {
        job,
        child,
        assigned: false,
        retired: false,
        entry: entry.clone(),
    };
    *entry
        .observation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = observation.clone();
    let assignment_job = owned.job.0;
    #[cfg(test)]
    let assignment_job = if request.fail_assignment {
        null_mut()
    } else {
        assignment_job
    };
    if unsafe { AssignProcessToJobObject(assignment_job, owned.child.process.0) } == 0 {
        return Err(Failure::Process);
    }
    owned.assigned = true;
    observation.assigned = true;
    let mut failure = None;
    let stop = Arc::new(AtomicBool::new(false));
    let state = Arc::new(AtomicU8::new(0));
    let reader_stop = stop.clone();
    let reader_state = state.clone();
    let reader = std::thread::Builder::new()
        .name("profile-export-pipe".into())
        .spawn(move || reader(input, limits.max_stdout, &reader_stop, &reader_state));
    let reader = match reader {
        Ok(reader) => reader,
        Err(_) => return Err(Failure::Owner),
    };
    let mut reader = ReaderOwner {
        thread: Some(reader),
        stop: stop.clone(),
        entry: entry.clone(),
    };
    observation.reader_started = true;
    *entry
        .observation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = observation.clone();
    if unsafe { ResumeThread(owned.child.thread.0) } == u32::MAX {
        failure = Some(Failure::Process);
    }
    #[cfg(test)]
    if request.panic_after_resume {
        panic!("synthetic profile owner unwind");
    }
    if output.close().is_err() | sink.close().is_err() {
        failure = Some(Failure::Cleanup);
    }
    let deadline = Instant::now() + limits.timeout;
    let mut terminated = false;
    loop {
        if entry.cancelled.load(Ordering::Acquire) {
            failure = Some(Failure::Cancelled);
        }
        if Instant::now() >= deadline && failure.is_none() {
            failure = Some(Failure::Timeout);
        }
        match state.load(Ordering::Acquire) {
            2 if failure.is_none() => failure = Some(Failure::Oversize),
            4 if failure.is_none() => failure = Some(Failure::Reader),
            _ => {}
        }
        let wait = unsafe { WaitForSingleObject(owned.child.process.0, 0) };
        if wait == WAIT_OBJECT_0 {
            let mut exit = 0;
            if unsafe { GetExitCodeProcess(owned.child.process.0, &mut exit) } == 0 {
                failure = Some(Failure::Cleanup);
            } else {
                observation.root_reaped = true;
                observation.exit_code = Some(exit);
            }
        } else if wait == WAIT_FAILED {
            failure = Some(Failure::Cleanup);
        }
        let empty = active(&owned.job);
        if let Ok(count) = empty {
            observation.active_process_peak = observation.active_process_peak.max(count);
        }
        if empty == Ok(0) && observation.root_reaped {
            observation.job_empty = true;
            owned.retired = true;
            break;
        }
        if empty.is_err() {
            failure = Some(Failure::Cleanup);
        }
        if (failure.is_some() || observation.root_reaped) && !terminated {
            if failure.is_some() {
                stop.store(true, Ordering::Release);
            }
            if unsafe { TerminateJobObject(owned.job.0, 1) } != 0 {
                terminated = true;
                observation.children_terminated = true;
            } else {
                failure = Some(Failure::Cleanup);
            }
        }
        match entry.observation.lock() {
            Ok(mut current) => *current = observation.clone(),
            Err(_) => failure = Some(Failure::Owner),
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if failure.is_some() {
        stop.store(true, Ordering::Release);
    }
    let joined = reader.thread.take().ok_or(Failure::Owner)?.join();
    observation.reader_joined = true;
    let (bytes, eof, pipe_closed) = joined.unwrap_or((Err(Failure::Reader), false, false));
    observation.pipe_eof = eof;
    if !pipe_closed {
        failure = Some(Failure::Cleanup);
    }
    owned.child.thread.close()?;
    owned.child.process.close()?;
    owned.job.close()?;
    observation.handles_closed = true;
    if entry.cancelled.load(Ordering::Acquire) {
        return Err(Failure::Cancelled);
    }
    if let Some(error) = failure {
        return Err(error);
    }
    if observation.exit_code != Some(0) || !eof {
        return Err(Failure::Process);
    }
    let bytes = bytes?;
    validate(&bytes, request.fixture)?;
    Ok(bytes)
}
