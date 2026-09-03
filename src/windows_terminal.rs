#[cfg(windows)]
mod windows_terminal {
    use std::ffi::OsString;
    use std::ffi::c_void;
    use std::io;
    use std::mem;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;
    use std::process::Command;

    type Bool = i32;
    type Dword = u32;
    type Handle = *mut c_void;

    const STD_OUTPUT_HANDLE: Dword = -11i32 as Dword;
    const STD_ERROR_HANDLE: Dword = -12i32 as Dword;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: Dword = 0x0004;
    const TH32CS_SNAPPROCESS: Dword = 0x00000002;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: Dword,
        cnt_usage: Dword,
        th32_process_id: Dword,
        th32_default_heap_id: usize,
        th32_module_id: Dword,
        cnt_threads: Dword,
        th32_parent_process_id: Dword,
        pc_pri_class_base: i32,
        dw_flags: Dword,
        sz_exe_file: [u16; 260],
    }

    unsafe extern "system" {
        fn GetStdHandle(n_std_handle: Dword) -> Handle;
        fn GetConsoleMode(h_console_handle: Handle, lp_mode: *mut Dword) -> Bool;
        fn SetConsoleMode(h_console_handle: Handle, dw_mode: Dword) -> Bool;
        fn GetCurrentProcessId() -> Dword;
        fn CreateToolhelp32Snapshot(dw_flags: Dword, th32_process_id: Dword) -> Handle;
        fn Process32FirstW(h_snapshot: Handle, lppe: *mut ProcessEntry32W) -> Bool;
        fn Process32NextW(h_snapshot: Handle, lppe: *mut ProcessEntry32W) -> Bool;
        fn CloseHandle(h_object: Handle) -> Bool;
    }

    pub fn enable_ansi_colors() {
        enable_ansi_for_handle(STD_OUTPUT_HANDLE);
        enable_ansi_for_handle(STD_ERROR_HANDLE);
    }

    pub fn parent_is_powershell() -> bool {
        parent_process_name()
            .map(|name| {
                let name = name.to_ascii_lowercase();
                name == "powershell.exe" || name == "pwsh.exe"
            })
            .unwrap_or(false)
    }

    pub fn launch_in_powershell(executable: &Path) -> io::Result<bool> {
        let executable = executable.to_string_lossy();
        let script = format!(
            "$env:V_FS_BACKUP_INSIDE_POWERSHELL='1'; & '{}'",
            executable.replace('\'', "''")
        );

        for shell in ["powershell.exe", "pwsh.exe"] {
            match Command::new(shell)
                .args([
                    "-NoLogo",
                    "-NoExit",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                ])
                .arg(&script)
                .spawn()
            {
                Ok(_) => return Ok(true),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }

        Ok(false)
    }

    fn enable_ansi_for_handle(handle_id: Dword) {
        unsafe {
            let handle = GetStdHandle(handle_id);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return;
            }

            let mut mode = 0;
            if GetConsoleMode(handle, &mut mode) == 0 {
                return;
            }

            let _ = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }

    fn parent_process_name() -> Option<String> {
        unsafe {
            let current_pid = GetCurrentProcessId();
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }

            let mut entry: ProcessEntry32W = mem::zeroed();
            entry.dw_size = mem::size_of::<ProcessEntry32W>() as Dword;

            let mut parent_pid = None;
            let mut ok = Process32FirstW(snapshot, &mut entry);
            while ok != 0 {
                if entry.th32_process_id == current_pid {
                    parent_pid = Some(entry.th32_parent_process_id);
                    break;
                }
                ok = Process32NextW(snapshot, &mut entry);
            }

            let Some(parent_pid) = parent_pid else {
                CloseHandle(snapshot);
                return None;
            };

            entry = mem::zeroed();
            entry.dw_size = mem::size_of::<ProcessEntry32W>() as Dword;
            ok = Process32FirstW(snapshot, &mut entry);
            while ok != 0 {
                if entry.th32_process_id == parent_pid {
                    let name = wide_array_to_string(&entry.sz_exe_file);
                    CloseHandle(snapshot);
                    return Some(name);
                }
                ok = Process32NextW(snapshot, &mut entry);
            }

            CloseHandle(snapshot);
            None
        }
    }

    fn wide_array_to_string(value: &[u16]) -> String {
        let len = value
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(value.len());
        OsString::from_wide(&value[..len])
            .to_string_lossy()
            .into_owned()
    }
}
