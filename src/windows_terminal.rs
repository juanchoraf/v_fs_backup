#[cfg(windows)]
mod windows_terminal {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    type Bool = i32;
    type Dword = u32;
    type Handle = *mut c_void;

    const STD_OUTPUT_HANDLE: Dword = -11i32 as Dword;
    const STD_ERROR_HANDLE: Dword = -12i32 as Dword;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: Dword = 0x0004;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const WM_SETICON: u32 = 0x0080;
    const ICON_SMALL: usize = 0;
    const ICON_BIG: usize = 1;

    unsafe extern "system" {
        fn GetStdHandle(n_std_handle: Dword) -> Handle;
        fn GetConsoleMode(h_console_handle: Handle, lp_mode: *mut Dword) -> Bool;
        fn SetConsoleMode(h_console_handle: Handle, dw_mode: Dword) -> Bool;
        fn GetConsoleWindow() -> Handle;
    }

    #[link(name = "Shell32")]
    unsafe extern "system" {
        fn ExtractIconExW(
            lpsz_file: *const u16,
            n_icon_index: i32,
            phicon_large: *mut Handle,
            phicon_small: *mut Handle,
            n_icons: u32,
        ) -> u32;
    }

    #[link(name = "User32")]
    unsafe extern "system" {
        fn SendMessageW(hwnd: Handle, msg: u32, w_param: usize, l_param: isize) -> isize;
    }

    pub fn enable_ansi_colors() {
        enable_ansi_for_handle(STD_OUTPUT_HANDLE);
        enable_ansi_for_handle(STD_ERROR_HANDLE);
    }

    pub fn apply_console_icon(executable: &Path) {
        unsafe {
            let window = GetConsoleWindow();
            if window.is_null() {
                return;
            }

            let executable = path_to_wide(executable);
            let mut large_icon: Handle = ptr::null_mut();
            let mut small_icon: Handle = ptr::null_mut();
            if ExtractIconExW(
                executable.as_ptr(),
                0,
                &mut large_icon,
                &mut small_icon,
                1,
            ) == 0
            {
                return;
            }

            if !small_icon.is_null() {
                let _ = SendMessageW(window, WM_SETICON, ICON_SMALL, small_icon as isize);
            }
            if !large_icon.is_null() {
                let _ = SendMessageW(window, WM_SETICON, ICON_BIG, large_icon as isize);
            }
        }
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

    fn path_to_wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
}
