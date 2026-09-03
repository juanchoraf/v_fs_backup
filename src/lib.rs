use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::error::Error as StdError;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Sender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use v_concat::{v_concat, v_concat_eprintln, v_concat_println};

type Error = Box<dyn StdError + Send + Sync + 'static>;
type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
struct SimpleError(String);

impl std::fmt::Display for SimpleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl StdError for SimpleError {}

fn simple_error(message: impl Into<String>) -> Error {
    Box::new(SimpleError(message.into()))
}

pub fn print_error(error: &(dyn StdError + 'static)) {
    initialize_terminal();

    let message = error.to_string();
    let mut output = v_concat!("{} {}", color(ANSI_RED, "error:"), message);
    if let Some(hint) = error_hint(&message) {
        output.push_str("\n\n");
        output.push_str(hint);
    }

    let mut source = error.source();
    while let Some(cause) = source {
        output.push_str(&v_concat!(
            "\n  {} {cause}",
            color(ANSI_YELLOW, "caused by:")
        ));
        source = cause.source();
    }

    print_padded_stderr(output);
}

fn error_hint(message: &str) -> Option<&'static str> {
    if message.contains("--to is required") {
        return Some(
            "  Add --to with the destination path.\n\n  Example:\n    v_fs_backup --dir C:\\Users\\Alejandra\\Documents --to D:\\Backups\\documents.fsb",
        );
    }
    if message.contains("unknown option") {
        return Some("  Run v_fs_backup --help to see the supported options.");
    }
    if message.contains("requires a value") {
        return Some("  Add a value after the option, or use --help for examples.");
    }
    if message.contains("nothing matched") || message.contains("nothing to back up") {
        return Some(
            "  Check the source path or selector, then try again with an existing file or directory.",
        );
    }
    if message.contains("pass --overwrite") {
        return Some("  Use --overwrite only when replacing the existing output is OK.");
    }

    None
}

fn print_padded_stdout(message: impl AsRef<str>) {
    v_concat_println!("\n{}\n", message.as_ref());
}

fn print_padded_stderr(message: impl AsRef<str>) {
    v_concat_eprintln!("\n{}\n", message.as_ref());
}

fn initialize_terminal() {
    #[cfg(windows)]
    windows_terminal::enable_ansi_colors();
}

fn relaunch_interactive_in_powershell() -> Result<bool> {
    #[cfg(windows)]
    {
        if env::var_os("V_FS_BACKUP_INSIDE_POWERSHELL").is_some()
            || windows_terminal::parent_is_powershell()
        {
            return Ok(false);
        }

        let executable =
            env::current_exe().context("failed to locate the current v_fs_backup executable")?;
        return windows_terminal::launch_in_powershell(&executable)
            .map_err(|error| simple_error(format!("failed to open PowerShell: {error}")));
    }

    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

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

macro_rules! bail {
    ($($arg:tt)*) => {
        return Err(simple_error(format!($($arg)*)))
    };
}

trait Context<T> {
    fn context(self, message: impl Into<String>) -> Result<T>;
    fn with_context(self, message: impl FnOnce() -> String) -> Result<T>;
}

impl<T, E> Context<T> for std::result::Result<T, E>
where
    E: StdError + Send + Sync + 'static,
{
    fn context(self, message: impl Into<String>) -> Result<T> {
        self.map_err(|error| simple_error(format!("{}: {error}", message.into())))
    }

    fn with_context(self, message: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|error| simple_error(format!("{}: {error}", message())))
    }
}

impl<T> Context<T> for Option<T> {
    fn context(self, message: impl Into<String>) -> Result<T> {
        self.ok_or_else(|| simple_error(message.into()))
    }

    fn with_context(self, message: impl FnOnce() -> String) -> Result<T> {
        self.ok_or_else(|| simple_error(message()))
    }
}

const MAGIC: &[u8; 8] = b"FSBKP05\n";
const LEGACY_RLE_MAGIC: &[u8; 8] = b"FSBKP04\n";
const FORMAT_VERSION: u32 = 5;
const LEGACY_FORMAT_VERSION: u32 = 4;
const MIN_COMPRESSION_LEVEL: i32 = 0;
const MAX_COMPRESSION_LEVEL: i32 = 22;
const TAG_MANIFEST: u8 = 1;
const TAG_DIRECTORY: u8 = 2;
const TAG_SYMLINK: u8 = 3;
const TAG_FILE_DATA: u8 = 4;
const TAG_FILE_REF: u8 = 5;
const COPY_BUFFER_SIZE: usize = 1024 * 1024;
const ARCHIVE_SAVE_PROGRESS_INTERVAL: Duration = Duration::from_secs(2);
const ANSI_RESET: &str = "\x1b[0m";
const ANSI_GREEN: &str = "\x1b[1;32m";
const ANSI_BLUE: &str = "\x1b[1;34m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_WHITE: &str = "\x1b[37m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[1;31m";
const INTERACTIVE_PROMPT: &str = "v_fs_backup> ";
const ARCHIVE_EXTENSION: &str = "fsb";

#[derive(Debug, Clone)]
pub struct Cli {
    file: Vec<String>,

    dir: Vec<String>,

    regex: Vec<String>,

    exclude_file: Vec<String>,

    exclude_dir: Vec<String>,

    exclude_regex: Vec<String>,

    no_recursive: bool,

    to: PathBuf,

    restore: Option<PathBuf>,

    compression_level: i32,

    jobs: usize,

    overwrite: bool,

    quiet: bool,

    roots: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct BackupRequest {
    pub roots: Vec<PathBuf>,
    pub files: Vec<String>,
    pub dirs: Vec<String>,
    pub regexes: Vec<String>,
    pub exclude_files: Vec<String>,
    pub exclude_dirs: Vec<String>,
    pub exclude_regexes: Vec<String>,
    pub no_recursive: bool,
    pub to: PathBuf,
    pub compression_level: i32,
    pub jobs: usize,
    pub overwrite: bool,
    pub quiet: bool,
}

#[derive(Debug, Clone)]
pub struct RestoreRequest {
    pub archive: PathBuf,
    pub to: PathBuf,
    pub overwrite: bool,
    pub quiet: bool,
}

#[derive(Debug, Default, Clone)]
pub struct BackupStats {
    pub archive_path: PathBuf,
    pub entries: usize,
    pub files: usize,
    pub directories: usize,
    pub symlinks: usize,
    pub original_bytes: u64,
    pub stored_file_bytes: u64,
    pub deduplicated_bytes: u64,
    pub archive_bytes: u64,
}

#[derive(Debug, Default, Clone)]
pub struct RestoreStats {
    pub entries: usize,
    pub files: usize,
    pub directories: usize,
    pub symlinks: usize,
    pub restored_bytes: u64,
}

#[derive(Debug, Clone)]
struct Selectors {
    include_files: Vec<PathSpec>,
    include_dirs: Vec<PathSpec>,
    include_regexes: Vec<Regex>,
    exclude_files: Vec<PathSpec>,
    exclude_dirs: Vec<PathSpec>,
    exclude_regexes: Vec<Regex>,
}

#[derive(Debug, Clone)]
struct PathSpec {
    normalized: String,
    has_separator: bool,
    is_absolute: bool,
}

#[derive(Debug, Clone)]
struct Regex {
    pattern: String,
    case_insensitive: bool,
}

struct RegexBuilder {
    pattern: String,
    case_insensitive: bool,
}

#[derive(Debug, Clone)]
struct WalkRoot {
    start: PathBuf,
    archive_prefix: Option<String>,
    include_all: bool,
}

struct CountingWriter<W> {
    inner: W,
    bytes_written: Arc<AtomicU64>,
}

struct CountingReader<R> {
    inner: R,
    bytes_read: Arc<AtomicU64>,
}

#[cfg(test)]
struct RleEncoder<W> {
    inner: W,
    min_run: usize,
    literal: Vec<u8>,
    run_byte: Option<u8>,
    run_len: usize,
}

struct RleDecoder<R> {
    inner: R,
    pending: VecDeque<u8>,
    done: bool,
}

struct ArchiveProgressMonitor {
    enabled: bool,
    label: &'static str,
    subject: Option<String>,
    stop: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
    started: Instant,
    compressed_bytes: Arc<AtomicU64>,
}

const INTERACTIVE_COMMANDS: &[&str] = &[
    "backup",
    "compress",
    "decompress",
    "restore",
    "help",
    "version",
    "clear",
    "exit",
    "quit",
    "q",
    "--file",
    "--dir",
    "--regex",
    "--rx",
    "-rx",
    "--exclude-file",
    "--ef",
    "-ef",
    "--exclude-dir",
    "--ed",
    "-ed",
    "--exclude-regex",
    "--er",
    "-er",
    "--to",
    "--restore",
    "--compression-level",
    "--compresion-level",
    "--jobs",
    "--overwrite",
    "--quiet",
    "--no-recursive",
    "-n",
    "-nr",
    "--help",
    "-h",
    "--version",
    "-V",
];

#[derive(Debug, Clone, Copy)]
struct InteractiveHelper;

impl rustyline::completion::Completer for InteractiveHelper {
    type Candidate = rustyline::completion::Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let token = completion_token(line, pos);
        let pairs = if should_complete_command(line, &token) {
            command_completion_pairs(&token.unquoted)
        } else {
            path_completion_pairs(&token)
        };

        Ok((token.start, pairs))
    }
}

impl rustyline::hint::Hinter for InteractiveHelper {
    type Hint = String;
}

impl rustyline::highlight::Highlighter for InteractiveHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        let _ = default;
        if prompt == INTERACTIVE_PROMPT {
            Cow::Owned(format!("{ANSI_CYAN}{prompt}{ANSI_WHITE}"))
        } else {
            Cow::Borrowed(prompt)
        }
    }
}

impl rustyline::validate::Validator for InteractiveHelper {}

impl rustyline::Helper for InteractiveHelper {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionToken {
    start: usize,
    unquoted: String,
    quote: Option<char>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathCompletionPrefix {
    parent: PathBuf,
    partial: String,
    display_prefix: String,
    separator: char,
}

fn completion_token(line: &str, pos: usize) -> CompletionToken {
    let pos = previous_char_boundary(line, pos.min(line.len()));
    let before = &line[..pos];
    let mut token_start = 0;
    let mut content_start = 0;
    let mut quote = None;
    let mut token_quote = None;

    for (index, character) in before.char_indices() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if character.is_whitespace() || character == '=' => {
                token_start = index + character.len_utf8();
                content_start = token_start;
                token_quote = None;
            }
            None if character == '\'' || character == '"' => {
                if index == token_start {
                    content_start = index + character.len_utf8();
                    token_quote = Some(character);
                }
                quote = Some(character);
            }
            None => {}
        }
    }

    let raw = &before[token_start..];
    let unquoted = if let Some(open) = token_quote {
        let mut value = before[content_start..].to_owned();
        if value.ends_with(open) {
            value.pop();
        }
        value
    } else {
        raw.to_owned()
    };

    CompletionToken {
        start: token_start,
        unquoted,
        quote: token_quote,
    }
}

fn previous_char_boundary(line: &str, mut pos: usize) -> usize {
    while pos > 0 && !line.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn should_complete_command(line: &str, token: &CompletionToken) -> bool {
    line[..token.start].trim().is_empty() || token.unquoted.starts_with('-')
}

fn command_completion_pairs(prefix: &str) -> Vec<rustyline::completion::Pair> {
    let mut pairs = INTERACTIVE_COMMANDS
        .iter()
        .filter(|command| command.starts_with(prefix))
        .map(|command| rustyline::completion::Pair {
            display: (*command).to_owned(),
            replacement: format!("{command} "),
        })
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| left.display.cmp(&right.display));
    pairs
}

fn path_completion_pairs(token: &CompletionToken) -> Vec<rustyline::completion::Pair> {
    let prefix = split_completion_path(&token.unquoted);
    let Ok(entries) = fs::read_dir(&prefix.parent) else {
        return Vec::new();
    };

    let mut pairs = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name_matches_prefix(&name, &prefix.partial) {
                return None;
            }

            let suffix = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                prefix.separator.to_string()
            } else {
                String::new()
            };
            let path = format!("{}{}{}", prefix.display_prefix, name, suffix);
            Some(rustyline::completion::Pair {
                display: path.clone(),
                replacement: quote_path_completion(&path, token.quote),
            })
        })
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| left.display.cmp(&right.display));
    pairs.truncate(64);
    pairs
}

fn split_completion_path(prefix: &str) -> PathCompletionPrefix {
    let separator = preferred_path_separator(prefix);
    let Some(index) = prefix.rfind(|character| character == '/' || character == '\\') else {
        return PathCompletionPrefix {
            parent: PathBuf::from("."),
            partial: prefix.to_owned(),
            display_prefix: String::new(),
            separator,
        };
    };

    let display_prefix = prefix[..=index].to_owned();
    let parent = if index == 0 || is_windows_drive_root(&display_prefix) {
        PathBuf::from(&display_prefix)
    } else {
        expand_completion_parent(&prefix[..index])
    };

    PathCompletionPrefix {
        parent,
        partial: prefix[index + 1..].to_owned(),
        display_prefix,
        separator,
    }
}

fn preferred_path_separator(prefix: &str) -> char {
    match (prefix.rfind('/'), prefix.rfind('\\')) {
        (Some(slash), Some(backslash)) if backslash > slash => '\\',
        (Some(_), _) => '/',
        (_, Some(_)) => '\\',
        (None, None) => std::path::MAIN_SEPARATOR,
    }
}

fn is_windows_drive_root(prefix: &str) -> bool {
    let bytes = prefix.as_bytes();
    bytes.len() == 3 && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn expand_completion_parent(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }

    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }

    PathBuf::from(path)
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from).or_else(|| {
            let drive = env::var_os("HOMEDRIVE")?;
            let path = env::var_os("HOMEPATH")?;
            Some(PathBuf::from(format!(
                "{}{}",
                drive.to_string_lossy(),
                path.to_string_lossy()
            )))
        })
    }

    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn name_matches_prefix(name: &str, partial: &str) -> bool {
    if cfg!(windows) {
        name.to_ascii_lowercase()
            .starts_with(&partial.to_ascii_lowercase())
    } else {
        name.starts_with(partial)
    }
}

fn quote_path_completion(path: &str, quote: Option<char>) -> String {
    let needs_quotes = quote.is_some() || path.chars().any(char::is_whitespace);
    if !needs_quotes {
        return path.to_owned();
    }

    if quote == Some('\'') && !path.contains('\'') {
        return format!("'{path}'");
    }
    if quote == Some('"') && !path.contains('"') {
        return format!("\"{path}\"");
    }
    if !path.contains('"') {
        return format!("\"{path}\"");
    }
    if !path.contains('\'') {
        return format!("'{path}'");
    }

    path.to_owned()
}

#[derive(Debug, Clone)]
struct ScannedEntry {
    source_path: PathBuf,
    archive_path: String,
    kind: EntryKind,
    metadata: EntryMetadata,
    hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveCompression {
    Zstd,
    LegacyRle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataPlatform {
    Unix,
    Windows,
}

#[derive(Debug, Clone)]
struct ArchiveManifest {
    format_version: u32,
    created_unix_seconds: i64,
    source_os: String,
}

#[derive(Debug, Clone)]
struct EntryMetadata {
    path: String,
    kind: EntryKind,
    len: u64,
    readonly: bool,
    modified: Option<FileStamp>,
    accessed: Option<FileStamp>,
    created: Option<FileStamp>,
    #[cfg(unix)]
    unix: Option<UnixMetadata>,
    #[cfg(windows)]
    windows: Option<WindowsMetadata>,
}

#[derive(Debug, Clone, Copy)]
struct FileStamp {
    seconds: i64,
    nanos: u32,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct UnixMetadata {
    mode: u32,
    uid: u32,
    gid: u32,
    rdev: u64,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct WindowsMetadata {
    file_attributes: u32,
}

#[derive(Debug, Clone)]
struct SymlinkRecord {
    meta: EntryMetadata,
    target: String,
    target_is_dir: Option<bool>,
}

#[derive(Debug, Clone)]
struct FileDataRecord {
    meta: EntryMetadata,
    hash: String,
    data_len: u64,
}

#[derive(Debug, Clone)]
struct FileRefRecord {
    meta: EntryMetadata,
    hash: String,
    original_path: String,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            file: Vec::new(),
            dir: Vec::new(),
            regex: Vec::new(),
            exclude_file: Vec::new(),
            exclude_dir: Vec::new(),
            exclude_regex: Vec::new(),
            no_recursive: false,
            to: PathBuf::new(),
            restore: None,
            compression_level: 6,
            jobs: default_jobs(),
            overwrite: false,
            quiet: false,
            roots: Vec::new(),
        }
    }
}

impl Cli {
    pub fn parse_from(args: impl IntoIterator<Item = OsString>) -> Self {
        Self::try_parse_from(args)
            .expect("failed to parse v_fs_backup arguments")
            .expect("help/version arguments do not produce a runnable CLI")
    }

    pub fn try_parse_from(args: impl IntoIterator<Item = OsString>) -> Result<Option<Self>> {
        let args = normalize_cli_args(args);
        let mut cli = Self::default();
        let mut idx = 1;

        while idx < args.len() {
            let arg = &args[idx];
            let text = arg.to_string_lossy();
            if text == "--help" || text == "-h" {
                print_help();
                return Ok(None);
            }
            if text == "--version" || text == "-V" {
                print_padded_stdout(v_concat!("v_fs_backup {}", env!("CARGO_PKG_VERSION")));
                return Ok(None);
            }

            if text == "--overwrite" {
                cli.overwrite = true;
                idx += 1;
                continue;
            }
            if text == "--quiet" {
                cli.quiet = true;
                idx += 1;
                continue;
            }
            if text == "--no-recursive" || text == "-n" {
                cli.no_recursive = true;
                idx += 1;
                continue;
            }

            if let Some((flag, value)) = split_inline_flag(&text) {
                cli.apply_flag_value(flag, OsString::from(value))?;
                idx += 1;
                continue;
            }

            if is_value_flag(&text) {
                let value = args
                    .get(idx + 1)
                    .with_context(|| format!("{text} requires a value"))?
                    .clone();
                cli.apply_flag_value(&text, value)?;
                idx += 2;
                continue;
            }

            if text.starts_with('-') {
                bail!("unknown option {text}");
            }

            cli.roots.push(PathBuf::from(arg));
            idx += 1;
        }

        Ok(Some(cli))
    }

    fn apply_flag_value(&mut self, flag: &str, value: OsString) -> Result<()> {
        let text = value.to_string_lossy().to_string();
        match flag {
            "--file" => self.file.push(text),
            "--dir" => self.dir.push(text),
            "--regex" | "--rx" => self.regex.push(text),
            "--exclude-file" | "--ef" => self.exclude_file.push(text),
            "--exclude-dir" | "--ed" => self.exclude_dir.push(text),
            "--exclude-regex" | "--er" => self.exclude_regex.push(text),
            "--to" => self.to = PathBuf::from(value),
            "--restore" => self.restore = Some(PathBuf::from(value)),
            "--compression-level" | "--compresion-level" => {
                self.compression_level = text
                    .parse()
                    .with_context(|| format!("invalid {flag} value {text:?}"))?;
            }
            "--jobs" => {
                self.jobs = text
                    .parse()
                    .with_context(|| format!("invalid --jobs value {text:?}"))?;
            }
            _ => bail!("unknown option {flag}"),
        }
        Ok(())
    }
}

pub fn run_from_env() -> Result<()> {
    initialize_terminal();
    let args: Vec<OsString> = env::args_os().collect();
    if args.len() == 1 {
        if relaunch_interactive_in_powershell()? {
            return Ok(());
        }
        return run_interactive_shell();
    }

    let Some(cli) = Cli::try_parse_from(args)? else {
        return Ok(());
    };
    run(cli)
}

fn run_interactive_shell() -> Result<()> {
    print_interactive_banner();

    let config = rustyline::Config::builder()
        .color_mode(rustyline::ColorMode::Forced)
        .completion_type(rustyline::CompletionType::List)
        .build();
    let mut editor =
        rustyline::Editor::<InteractiveHelper, rustyline::history::DefaultHistory>::with_config(
            config,
        )
        .map_err(|error| simple_error(format!("failed to start interactive prompt: {error}")))?;
    editor.set_helper(Some(InteractiveHelper));
    let prompt = INTERACTIVE_PROMPT;

    loop {
        let line = match editor.readline(prompt) {
            Ok(line) => line,
            Err(rustyline::error::ReadlineError::Interrupted) => {
                print_padded_stderr(color(ANSI_YELLOW, "Command cancelled"));
                continue;
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                print_padded_stdout("");
                return Ok(());
            }
            Err(error) => bail!("failed to read command: {error}"),
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let _ = editor.add_history_entry(line);

        if matches!(line, "exit" | "quit" | "q") {
            return Ok(());
        }
        if line == "clear" {
            clear_interactive_screen()?;
            continue;
        }

        match interactive_args_from_line(line) {
            Ok(args) => match Cli::try_parse_from(args) {
                Ok(Some(cli)) => {
                    if let Err(error) = run(cli) {
                        print_error(&*error);
                    }
                }
                Ok(None) => {}
                Err(error) => print_error(&*error),
            },
            Err(error) => print_error(&*error),
        }
    }
}

fn clear_interactive_screen() -> Result<()> {
    // ANSI clear-screen and cursor-home sequences work on Unix terminals and
    // on Windows after initialize_terminal enables virtual terminal handling.
    print_padded_stdout("\x1b[2J\x1b[H");
    io::stdout()
        .flush()
        .context("failed to clear the interactive console")?;
    print_interactive_banner();
    Ok(())
}

const BANNER_COMPRESSION_INPUT: &str = r#"    .======================.
   ||  HUGE FILES / DIRS   ||
   || ++++++++++++++++++++ ||
    '==========\/=========='
               \/
"#;

const BANNER_COMPRESSION_OUTPUT: &str = r#"         .-------------.
         |  tiny .fsb  |
         '-------------' "#;

// Keep this raw ASCII block exactly as it should appear in the terminal so the
// banner can be edited here without translating backslashes into Rust escapes.
const BANNER_WORDMARK: &str = r#"
 __     __          _____ ____           ____    _    ____ _  ___   _ ____
 \ \   / /         |  ___/ ___|         | __ )  / \  / ___| |/ / | | |  _ \
  \ \ / /          | |_  \___ \         |  _ \ / _ \| |   | ' /| | | | |_) |
   \ V /  ______   |  _|  ___) | ______ | |_) / ___ \ |___| . \| |_| |  __/
    \_/  |______|  |_|   |____/ |______||____/_/   \_\____|_|\_\\___/|_|

"#;

fn print_interactive_banner() {
    print_padded_stdout(v_concat!(
        "{ANSI_CYAN}{BANNER_COMPRESSION_INPUT}{ANSI_YELLOW}{BANNER_COMPRESSION_OUTPUT}{ANSI_BLUE}{BANNER_WORDMARK}{ANSI_RESET}\
v_fs_backup: fast compressed filesystem backups. Type help, compress, decompress, clear, or exit."
    ));
}

fn interactive_args_from_line(line: &str) -> Result<Vec<OsString>> {
    let words = split_interactive_line(line)?;
    let mut args = vec![OsString::from("v_fs_backup")];
    if words.is_empty() {
        return Ok(args);
    }

    match words[0].as_str() {
        "help" => args.push(OsString::from("--help")),
        "version" => args.push(OsString::from("--version")),
        "compress" if words.len() == 3 => {
            args.push(OsString::from("--dir"));
            args.push(OsString::from(&words[1]));
            args.push(OsString::from("--to"));
            args.push(OsString::from(&words[2]));
        }
        "decompress" | "restore" if words.len() == 3 => {
            args.push(OsString::from("--restore"));
            args.push(OsString::from(&words[1]));
            args.push(OsString::from("--to"));
            args.push(OsString::from(&words[2]));
        }
        "backup" | "compress" | "decompress" | "restore" => {
            args.extend(words.into_iter().skip(1).map(OsString::from));
        }
        _ => args.extend(words.into_iter().map(OsString::from)),
    }

    Ok(args)
}

fn split_interactive_line(line: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;

    for character in line.chars() {
        if let Some(quote_character) = quote {
            if escaped {
                if character == quote_character || character == '\\' {
                    current.push(character);
                } else {
                    current.push('\\');
                    current.push(character);
                }
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }

        if character == '"' || character == '\'' {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }

    if escaped {
        current.push('\\');
    }
    if quote.is_some() {
        bail!("unterminated quote in command");
    }
    if !current.is_empty() {
        words.push(current);
    }

    Ok(words)
}

pub fn run(cli: Cli) -> Result<()> {
    let started = Instant::now();
    validate_cli(&cli)?;
    let quiet = cli.quiet;

    if let Some(archive) = cli.restore.clone() {
        let stats = restore_archive(RestoreRequest {
            archive,
            to: cli.to,
            overwrite: cli.overwrite,
            quiet,
        })?;
        if !quiet {
            print_restore_summary(&stats, started.elapsed());
        }
        return Ok(());
    }

    let stats = create_backup(BackupRequest {
        roots: cli.roots,
        files: cli.file,
        dirs: cli.dir,
        regexes: cli.regex,
        exclude_files: cli.exclude_file,
        exclude_dirs: cli.exclude_dir,
        exclude_regexes: cli.exclude_regex,
        no_recursive: cli.no_recursive,
        to: cli.to,
        compression_level: cli.compression_level,
        jobs: cli.jobs,
        overwrite: cli.overwrite,
        quiet,
    })?;

    if !quiet && !stats.archive_path.as_os_str().is_empty() {
        print_backup_summary(&stats, started.elapsed());
    }

    Ok(())
}

pub fn create_backup(request: BackupRequest) -> Result<BackupStats> {
    let prepare_started = Instant::now();
    validate_backup_request(&request)?;
    let selectors = Selectors::new(&request)?;
    let walk_roots = build_walk_roots(&request, &selectors)?;
    if walk_roots.is_empty() {
        bail!("nothing to back up: no roots or selectors resolved to readable paths");
    }
    if !request.quiet {
        print_time_row("Prepare", prepare_started.elapsed());
    }

    if !request.quiet {
        print_padded_stderr(v_concat!(
            "Scanning {} root(s) with {} worker(s)...",
            walk_roots.len(),
            request.jobs
        ));
    }

    let scan_started = Instant::now();
    let mut entries =
        collect_entries(&walk_roots, &selectors, request.no_recursive, request.quiet)?;
    if !request.quiet {
        print_time_row("Scan", scan_started.elapsed());
    }
    if entries.is_empty() {
        bail!("nothing matched the backup request");
    }

    if !request.quiet {
        print_padded_stderr(v_concat!("Hashing {} selected entrie(s)...", entries.len()));
    }
    let hash_started = Instant::now();
    hash_files(&mut entries, request.jobs)?;
    if !request.quiet {
        print_time_row("Hash/Read", hash_started.elapsed());
    }

    let copy_compress_started = Instant::now();
    let archive_path = resolve_backup_output_path(&request.to)?;
    if let Some(parent) = archive_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create archive directory {}", parent.display()))?;
    }

    let output = OpenOptions::new()
        .write(true)
        .create(request.overwrite)
        .create_new(!request.overwrite)
        .truncate(request.overwrite)
        .open(&archive_path)
        .with_context(|| {
            if request.overwrite {
                format!("failed to open archive {}", archive_path.display())
            } else {
                format!(
                    "failed to create archive {}; pass --overwrite to replace it",
                    archive_path.display()
                )
            }
        })?;

    let mut writer = BufWriter::new(output);
    writer.write_all(MAGIC)?;
    let compressed_bytes = Arc::new(AtomicU64::new(MAGIC.len() as u64));
    let counting_writer = CountingWriter::new(writer, Arc::clone(&compressed_bytes));

    let mut encoder = zstd::stream::write::Encoder::new(counting_writer, request.compression_level)
        .context("failed to create zstd encoder")?;
    encoder
        .include_checksum(true)
        .context("failed to configure zstd checksum")?;

    write_json_record(
        &mut encoder,
        TAG_MANIFEST,
        manifest_to_value(&ArchiveManifest {
            format_version: FORMAT_VERSION,
            created_unix_seconds: now_unix_seconds(),
            source_os: env::consts::OS.to_string(),
        }),
    )?;

    let mut seen_hashes: HashMap<String, String> = HashMap::new();
    let mut stats = BackupStats {
        archive_path: archive_path.clone(),
        entries: entries.len(),
        ..BackupStats::default()
    };

    for entry in entries {
        match entry.kind {
            EntryKind::Directory => {
                stats.directories += 1;
                if !request.quiet {
                    print_padded_stderr(v_concat!("Copy Dir {}", entry.archive_path));
                }
                write_json_record(
                    &mut encoder,
                    TAG_DIRECTORY,
                    entry_metadata_to_value(&entry.metadata),
                )?;
            }
            EntryKind::Symlink => {
                stats.symlinks += 1;
                let target = fs::read_link(&entry.source_path).with_context(|| {
                    format!("failed to read symlink {}", entry.source_path.display())
                })?;
                if !request.quiet {
                    print_padded_stderr(v_concat!(
                        "Copy Link {} -> {}",
                        entry.archive_path,
                        target.display()
                    ));
                }
                let target_is_dir = fs::metadata(&entry.source_path).ok().map(|m| m.is_dir());
                write_json_record(
                    &mut encoder,
                    TAG_SYMLINK,
                    symlink_record_to_value(&SymlinkRecord {
                        meta: entry.metadata,
                        target: normalize_path_lossy(&target),
                        target_is_dir,
                    }),
                )?;
            }
            EntryKind::File => {
                stats.files += 1;
                stats.original_bytes += entry.metadata.len;
                let hash = entry
                    .hash
                    .clone()
                    .context("internal error: selected file was not hashed")?;
                if let Some(original_path) = seen_hashes.get(&hash) {
                    stats.deduplicated_bytes += entry.metadata.len;
                    if !request.quiet {
                        print_sized_progress(
                            "Copy File",
                            entry.metadata.len,
                            &entry.archive_path,
                            Some(&format!("duplicate of {original_path}")),
                        );
                    }
                    write_json_record(
                        &mut encoder,
                        TAG_FILE_REF,
                        file_ref_record_to_value(&FileRefRecord {
                            meta: entry.metadata,
                            hash,
                            original_path: original_path.clone(),
                        }),
                    )?;
                } else {
                    seen_hashes.insert(hash.clone(), entry.archive_path.clone());
                    stats.stored_file_bytes += entry.metadata.len;
                    if !request.quiet {
                        print_sized_progress(
                            "Copy File",
                            entry.metadata.len,
                            &entry.archive_path,
                            None,
                        );
                    }
                    write_json_record(
                        &mut encoder,
                        TAG_FILE_DATA,
                        file_data_record_to_value(&FileDataRecord {
                            meta: entry.metadata.clone(),
                            hash,
                            data_len: entry.metadata.len,
                        }),
                    )?;
                    let mut input = File::open(&entry.source_path).with_context(|| {
                        format!("failed to open file {}", entry.source_path.display())
                    })?;
                    copy_exact_bytes(&mut input, &mut encoder, entry.metadata.len)?;
                }
            }
        }
    }
    if !request.quiet {
        print_time_row("Copy/Compress", copy_compress_started.elapsed());
    }
    let save_archive_started = Instant::now();
    let save_monitor = ArchiveProgressMonitor::start(
        !request.quiet,
        "Deflating",
        None,
        Arc::clone(&compressed_bytes),
    );
    let mut writer = match encoder.finish().context("failed to finish zstd stream") {
        Ok(writer) => writer,
        Err(error) => {
            save_monitor.finish();
            return Err(error);
        }
    };
    let flush_result = writer.flush().context("failed to flush archive");
    save_monitor.finish();
    flush_result?;
    if !request.quiet {
        print_time_row("Save Archive", save_archive_started.elapsed());
    }
    stats.archive_bytes = fs::metadata(&archive_path)
        .with_context(|| format!("failed to stat archive {}", archive_path.display()))?
        .len();
    Ok(stats)
}

pub fn restore_archive(request: RestoreRequest) -> Result<RestoreStats> {
    if request.archive.as_os_str().is_empty() {
        bail!("--restore requires an archive path");
    }
    fs::create_dir_all(&request.to).with_context(|| {
        format!(
            "failed to create restore directory {}",
            request.to.display()
        )
    })?;

    let archive = File::open(&request.archive)
        .with_context(|| format!("failed to open archive {}", request.archive.display()))?;
    let mut reader = BufReader::new(archive);
    let mut magic = [0_u8; MAGIC.len()];
    reader
        .read_exact(&mut magic)
        .with_context(|| format!("{} is not a v_fs_backup archive", request.archive.display()))?;
    let compression = match &magic {
        MAGIC => ArchiveCompression::Zstd,
        LEGACY_RLE_MAGIC => ArchiveCompression::LegacyRle,
        _ => bail!("{} is not a v_fs_backup archive", request.archive.display()),
    };

    let compressed_bytes = Arc::new(AtomicU64::new(magic.len() as u64));
    let counting_reader = CountingReader::new(reader, Arc::clone(&compressed_bytes));
    let mut decoder: Box<dyn Read> = match compression {
        ArchiveCompression::Zstd => Box::new(
            zstd::stream::read::Decoder::new(counting_reader)
                .context("failed to create zstd decoder")?,
        ),
        ArchiveCompression::LegacyRle => Box::new(RleDecoder::new(counting_reader)),
    };
    let inflate_monitor = ArchiveProgressMonitor::start(
        !request.quiet,
        "Inflating",
        Some(progress_file_name(&request.archive)),
        Arc::clone(&compressed_bytes),
    );
    let mut restored_by_hash: HashMap<String, PathBuf> = HashMap::new();
    let mut directories_for_metadata = Vec::new();
    let mut stats = RestoreStats::default();
    let mut archive_source_os = None;

    while let Some((tag, json)) = read_json_record(&mut decoder)? {
        match tag {
            TAG_MANIFEST => {
                let manifest = manifest_from_slice(&json)?;
                let supported_version = manifest.format_version == FORMAT_VERSION
                    || (compression == ArchiveCompression::LegacyRle
                        && manifest.format_version == LEGACY_FORMAT_VERSION);
                if !supported_version {
                    bail!(
                        "unsupported archive format version {}; this binary supports {}",
                        manifest.format_version,
                        FORMAT_VERSION
                    );
                }
                archive_source_os = Some(manifest.source_os);
            }
            TAG_DIRECTORY => {
                let meta = entry_metadata_from_slice_for_os(
                    &json,
                    require_archive_source_os(&archive_source_os)?,
                )?;
                let target = safe_restore_path(&request.to, &meta.path)?;
                if !request.quiet {
                    print_padded_stderr(v_concat!("Restore Dir {}", target.display()));
                }
                fs::create_dir_all(&target)
                    .with_context(|| format!("failed to create directory {}", target.display()))?;
                directories_for_metadata.push((target, meta));
                stats.directories += 1;
                stats.entries += 1;
            }
            TAG_SYMLINK => {
                let record = symlink_record_from_slice_for_os(
                    &json,
                    require_archive_source_os(&archive_source_os)?,
                )?;
                let target = safe_restore_path(&request.to, &record.meta.path)?;
                create_parent_dir(&target)?;
                prepare_restore_target(&target, request.overwrite)?;
                if !request.quiet {
                    print_padded_stderr(v_concat!("Restore Link {}", target.display()));
                }
                if let Err(error) =
                    create_symlink(Path::new(&record.target), &target, record.target_is_dir)
                {
                    bail!("failed to restore symlink {}: {error}", target.display());
                }
                stats.symlinks += 1;
                stats.entries += 1;
            }
            TAG_FILE_DATA => {
                let record = file_data_record_from_slice_for_os(
                    &json,
                    require_archive_source_os(&archive_source_os)?,
                )?;
                let target = safe_restore_path(&request.to, &record.meta.path)?;
                create_parent_dir(&target)?;
                prepare_restore_target(&target, request.overwrite)?;
                if !request.quiet {
                    print_sized_progress(
                        "Restore File",
                        record.data_len,
                        &target.display().to_string(),
                        None,
                    );
                }
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .with_context(|| format!("failed to create file {}", target.display()))?;
                copy_exact_bytes(&mut decoder, &mut output, record.data_len)?;
                output
                    .flush()
                    .with_context(|| format!("failed to flush {}", target.display()))?;
                apply_metadata(&target, &record.meta)?;
                restored_by_hash.insert(record.hash, target);
                stats.files += 1;
                stats.entries += 1;
                stats.restored_bytes += record.data_len;
            }
            TAG_FILE_REF => {
                let record = file_ref_record_from_slice_for_os(
                    &json,
                    require_archive_source_os(&archive_source_os)?,
                )?;
                let target = safe_restore_path(&request.to, &record.meta.path)?;
                create_parent_dir(&target)?;
                prepare_restore_target(&target, request.overwrite)?;
                let original = restored_by_hash.get(&record.hash).with_context(|| {
                    format!(
                        "archive references duplicate file {} before original {}",
                        record.meta.path, record.original_path
                    )
                })?;
                if !request.quiet {
                    print_sized_progress(
                        "Restore File",
                        record.meta.len,
                        &target.display().to_string(),
                        Some("duplicate"),
                    );
                }
                let mut input = File::open(original).with_context(|| {
                    format!("failed to open duplicate file {}", original.display())
                })?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .with_context(|| format!("failed to create file {}", target.display()))?;
                copy_exact_bytes(&mut input, &mut output, record.meta.len)?;
                output
                    .flush()
                    .with_context(|| format!("failed to flush {}", target.display()))?;
                apply_metadata(&target, &record.meta)?;
                stats.files += 1;
                stats.entries += 1;
                stats.restored_bytes += record.meta.len;
            }
            other => bail!("unknown archive record tag {other}"),
        }
    }

    directories_for_metadata.sort_by_key(|entry| Reverse(entry.0.components().count()));
    for (path, meta) in directories_for_metadata {
        apply_metadata(&path, &meta)?;
    }

    inflate_monitor.finish();
    Ok(stats)
}

pub fn normalize_cli_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut normalized = Vec::new();
    'args: for arg in args {
        if let Some(text) = arg.to_str() {
            match text {
                "-nr" => {
                    normalized.push(OsString::from("--no-recursive"));
                    continue 'args;
                }
                "-rx" => {
                    normalized.push(OsString::from("--regex"));
                    continue 'args;
                }
                "-ef" => {
                    normalized.push(OsString::from("--exclude-file"));
                    continue 'args;
                }
                "-ed" => {
                    normalized.push(OsString::from("--exclude-dir"));
                    continue 'args;
                }
                "-er" => {
                    normalized.push(OsString::from("--exclude-regex"));
                    continue 'args;
                }
                _ => {}
            }
            for (alias, long) in [
                ("-rx=", "--regex="),
                ("-ef=", "--exclude-file="),
                ("-ed=", "--exclude-dir="),
                ("-er=", "--exclude-regex="),
            ] {
                if let Some(value) = text.strip_prefix(alias) {
                    normalized.push(OsString::from(format!("{long}{value}")));
                    continue 'args;
                }
            }
        }
        normalized.push(arg);
    }
    normalized
}

fn split_inline_flag(text: &str) -> Option<(&str, &str)> {
    let (flag, value) = text.split_once('=')?;
    is_value_flag(flag).then_some((flag, value))
}

fn is_value_flag(text: &str) -> bool {
    matches!(
        text,
        "--file"
            | "--dir"
            | "--regex"
            | "--rx"
            | "--exclude-file"
            | "--ef"
            | "--exclude-dir"
            | "--ed"
            | "--exclude-regex"
            | "--er"
            | "--to"
            | "--restore"
            | "--compression-level"
            | "--compresion-level"
            | "--jobs"
    )
}

fn print_help() {
    print_padded_stdout(v_concat!(
        "v_fs_backup {}\n\nFast, compressed, metadata-preserving filesystem backups.\n\nUSAGE:\n  v_fs_backup [OPTIONS] [SEARCH_ROOT ...] --to <ARCHIVE_OR_DIRECTORY>\n  v_fs_backup --restore <ARCHIVE> --to <RESTORE_DIRECTORY>\n\nOPTIONS:\n  --file <PATH_OR_NAME>             Back up a matching file\n  --dir <PATH_OR_NAME>              Back up a matching directory\n  --regex, --rx <REGEX>             Back up paths matching a regex\n  --exclude-file, --ef <PATH>       Exclude a file\n  --exclude-dir, --ed <PATH>        Exclude a directory tree\n  --exclude-regex, --er <REGEX>     Exclude paths matching a regex\n  -n, --no-recursive                Do not recurse into subdirectories\n  --to <PATH>                       Archive path for backups or restore directory\n  --restore <ARCHIVE>               Restore a v_fs_backup archive\n  --compression-level <0..22>       zstd compression level, default 6\n  --jobs <N>                        Hashing worker count\n  --overwrite                       Replace an existing archive or restore target\n  --quiet                           Suppress progress output\n  -h, --help                        Show this help\n  -V, --version                     Show version\n\nCompatibility aliases accepted before parsing: -nr, -rx, -ef, -ed, -er, --compresion-level.",
        env!("CARGO_PKG_VERSION")
    ));
}

impl Regex {
    fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            case_insensitive: false,
        }
    }

    fn is_match(&self, value: &str) -> bool {
        let mut pattern = self.pattern.clone();
        let mut value = value.to_string();
        if self.case_insensitive {
            pattern = pattern.to_lowercase();
            value = value.to_lowercase();
        }

        if let Some(extension) = regex_extension_hint(&pattern) {
            return value.contains(&extension);
        }

        let anchored_start = pattern.starts_with('^');
        let anchored_end = pattern.ends_with('$') && !pattern.ends_with("\\$");
        if anchored_start {
            pattern.remove(0);
        }
        if anchored_end {
            pattern.pop();
        }

        let literal = regex_literal_hint(&pattern);
        if literal.contains('*') || literal.contains('?') {
            return wildcard_match(&literal, &value);
        }
        if anchored_start && anchored_end {
            value == literal
        } else if anchored_start {
            value.starts_with(&literal)
        } else if anchored_end {
            value.ends_with(&literal)
        } else {
            value.contains(&literal)
        }
    }
}

impl RegexBuilder {
    fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            case_insensitive: false,
        }
    }

    fn case_insensitive(&mut self, enabled: bool) -> &mut Self {
        self.case_insensitive = enabled;
        self
    }

    fn multi_line(&mut self, _enabled: bool) -> &mut Self {
        self
    }

    fn dot_matches_new_line(&mut self, _enabled: bool) -> &mut Self {
        self
    }

    fn ignore_whitespace(&mut self, _enabled: bool) -> &mut Self {
        self
    }

    fn swap_greed(&mut self, _enabled: bool) -> &mut Self {
        self
    }

    fn build(&self) -> Regex {
        Regex {
            pattern: self.pattern.clone(),
            case_insensitive: self.case_insensitive,
        }
    }
}

impl Selectors {
    fn new(request: &BackupRequest) -> Result<Self> {
        Ok(Self {
            include_files: request.files.iter().map(|s| PathSpec::new(s)).collect(),
            include_dirs: request.dirs.iter().map(|s| PathSpec::new(s)).collect(),
            include_regexes: compile_regexes(&request.regexes)?,
            exclude_files: request
                .exclude_files
                .iter()
                .map(|s| PathSpec::new(s))
                .collect(),
            exclude_dirs: request
                .exclude_dirs
                .iter()
                .map(|s| PathSpec::new(s))
                .collect(),
            exclude_regexes: compile_regexes(&request.exclude_regexes)?,
        })
    }

    fn has_include_filters(&self) -> bool {
        !(self.include_files.is_empty()
            && self.include_dirs.is_empty()
            && self.include_regexes.is_empty())
    }

    fn matches_include_file(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        self.include_files
            .iter()
            .any(|spec| spec.matches(archive_path, absolute_path, name))
    }

    fn matches_include_dir(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        self.include_dirs
            .iter()
            .any(|spec| spec.matches(archive_path, absolute_path, name))
    }

    fn matches_include_regex(&self, archive_path: &str, absolute_path: &Path) -> bool {
        let absolute = normalize_path_lossy(absolute_path);
        self.include_regexes
            .iter()
            .any(|rx| rx.is_match(archive_path) || rx.is_match(&absolute))
    }

    fn excludes_file(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        self.exclude_files
            .iter()
            .any(|spec| spec.matches(archive_path, absolute_path, name))
    }

    fn excludes_dirish(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        let absolute = normalize_path_lossy(absolute_path);
        self.exclude_dirs
            .iter()
            .any(|spec| spec.matches_dir_tree(archive_path, &absolute, name))
            || self
                .exclude_regexes
                .iter()
                .any(|rx| rx.is_match(archive_path) || rx.is_match(&absolute))
    }

    fn excludes_regex(&self, archive_path: &str, absolute_path: &Path) -> bool {
        let absolute = normalize_path_lossy(absolute_path);
        self.exclude_regexes
            .iter()
            .any(|rx| rx.is_match(archive_path) || rx.is_match(&absolute))
    }
}

impl PathSpec {
    fn new(raw: &str) -> Self {
        let path = Path::new(raw);
        let has_separator = raw.contains('/') || raw.contains('\\');
        Self {
            normalized: normalize_match_text(raw),
            has_separator,
            is_absolute: path.is_absolute(),
        }
    }

    fn matches(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        if self.has_separator || self.is_absolute {
            let archive = normalize_match_text(archive_path);
            let absolute = normalize_match_text(&normalize_path_lossy(absolute_path));
            path_eq_or_nested_match(&archive, &self.normalized)
                || path_eq_or_nested_match(&absolute, &self.normalized)
        } else {
            normalize_match_text(name) == self.normalized
        }
    }

    fn matches_dir_tree(&self, archive_path: &str, absolute_path: &str, name: &str) -> bool {
        if self.matches(archive_path, Path::new(absolute_path), name) {
            return true;
        }
        if self.has_separator || self.is_absolute {
            let archive = normalize_match_text(archive_path);
            let absolute = normalize_match_text(absolute_path);
            path_contains_component_path(&archive, &self.normalized)
                || path_contains_component_path(&absolute, &self.normalized)
        } else {
            normalize_match_text(archive_path)
                .split('/')
                .any(|part| part == self.normalized)
        }
    }
}

impl<W> CountingWriter<W> {
    fn new(inner: W, bytes_written: Arc<AtomicU64>) -> Self {
        Self {
            inner,
            bytes_written,
        }
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.bytes_written
            .fetch_add(written as u64, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<R> CountingReader<R> {
    fn new(inner: R, bytes_read: Arc<AtomicU64>) -> Self {
        Self { inner, bytes_read }
    }
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.bytes_read.fetch_add(read as u64, Ordering::Relaxed);
        Ok(read)
    }
}

#[cfg(test)]
impl<W: Write> RleEncoder<W> {
    fn new(inner: W, compression_level: i32) -> Self {
        let min_run = match compression_level {
            0 => usize::MAX,
            1..=3 => 4,
            _ => 3,
        };
        Self {
            inner,
            min_run,
            literal: Vec::with_capacity(128),
            run_byte: None,
            run_len: 0,
        }
    }

    fn finish(mut self) -> io::Result<W> {
        self.flush_run()?;
        self.flush_literal()?;
        self.inner.flush()?;
        Ok(self.inner)
    }

    fn push_byte(&mut self, byte: u8) -> io::Result<()> {
        match self.run_byte {
            Some(run_byte) if run_byte == byte && self.run_len < 128 => {
                self.run_len += 1;
            }
            Some(_) => {
                self.flush_run()?;
                self.run_byte = Some(byte);
                self.run_len = 1;
            }
            None => {
                self.run_byte = Some(byte);
                self.run_len = 1;
            }
        }
        Ok(())
    }

    fn flush_run(&mut self) -> io::Result<()> {
        let Some(run_byte) = self.run_byte.take() else {
            return Ok(());
        };
        let run_len = self.run_len;
        self.run_len = 0;

        if run_len >= self.min_run {
            self.flush_literal()?;
            self.inner.write_all(&[(127 + run_len) as u8, run_byte])?;
        } else {
            for _ in 0..run_len {
                self.literal.push(run_byte);
                if self.literal.len() == 128 {
                    self.flush_literal()?;
                }
            }
        }
        Ok(())
    }

    fn flush_literal(&mut self) -> io::Result<()> {
        if self.literal.is_empty() {
            return Ok(());
        }
        self.inner.write_all(&[(self.literal.len() - 1) as u8])?;
        self.inner.write_all(&self.literal)?;
        self.literal.clear();
        Ok(())
    }
}

#[cfg(test)]
impl<W: Write> Write for RleEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for byte in buf {
            self.push_byte(*byte)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_run()?;
        self.flush_literal()?;
        self.inner.flush()
    }
}

impl<R: Read> RleDecoder<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            pending: VecDeque::new(),
            done: false,
        }
    }

    fn fill_pending(&mut self) -> io::Result<()> {
        if self.done || !self.pending.is_empty() {
            return Ok(());
        }

        let mut control = [0_u8; 1];
        match self.inner.read_exact(&mut control) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                self.done = true;
                return Ok(());
            }
            Err(error) => return Err(error),
        }

        match control[0] {
            0..=127 => {
                let len = control[0] as usize + 1;
                let mut literal = vec![0_u8; len];
                self.inner.read_exact(&mut literal)?;
                self.pending.extend(literal);
            }
            128 => {}
            encoded_run => {
                let len = encoded_run as usize - 127;
                let mut byte = [0_u8; 1];
                self.inner.read_exact(&mut byte)?;
                self.pending.extend(std::iter::repeat_n(byte[0], len));
            }
        }

        Ok(())
    }
}

impl<R: Read> Read for RleDecoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        while self.pending.is_empty() && !self.done {
            self.fill_pending()?;
        }

        let mut written = 0;
        while written < buf.len() {
            let Some(byte) = self.pending.pop_front() else {
                break;
            };
            buf[written] = byte;
            written += 1;
        }
        Ok(written)
    }
}

impl ArchiveProgressMonitor {
    fn start(
        enabled: bool,
        label: &'static str,
        subject: Option<String>,
        compressed_bytes: Arc<AtomicU64>,
    ) -> Self {
        let started = Instant::now();
        let (stop, stop_rx) = mpsc::channel();
        let handle = enabled.then(|| {
            let compressed_bytes = Arc::clone(&compressed_bytes);
            let subject = subject.clone();
            thread::spawn(move || {
                while stop_rx
                    .recv_timeout(ARCHIVE_SAVE_PROGRESS_INTERVAL)
                    .is_err()
                {
                    print_archive_progress(
                        label,
                        subject.as_deref(),
                        compressed_bytes.load(Ordering::Relaxed),
                        started.elapsed(),
                    );
                }
            })
        });

        Self {
            enabled,
            label,
            subject,
            stop: enabled.then_some(stop),
            handle,
            started,
            compressed_bytes,
        }
    }

    fn finish(mut self) {
        if !self.enabled {
            return;
        }
        self.stop_thread();
        print_archive_progress(
            self.label,
            self.subject.as_deref(),
            self.compressed_bytes.load(Ordering::Relaxed),
            self.started.elapsed(),
        );
    }

    fn stop_thread(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ArchiveProgressMonitor {
    fn drop(&mut self) {
        if self.enabled {
            self.stop_thread();
        }
    }
}

fn print_archive_progress(
    label: &str,
    subject: Option<&str>,
    compressed_bytes: u64,
    elapsed: Duration,
) {
    let mut output = color(ANSI_CYAN, label);
    if let Some(subject) = subject {
        output.push('\n');
        output.push_str(&fact_line("Archive", subject));
        output.push('\n');
        output.push_str(&fact_line("Read", human_bytes(compressed_bytes)));
    } else {
        output.push('\n');
        output.push_str(&fact_line(
            "Compressed archive",
            human_bytes(compressed_bytes),
        ));
    }
    output.push('\n');
    output.push_str(&fact_line("Elapsed", human_duration(elapsed)));
    print_padded_stderr(output);
}

fn progress_file_name(path: &Path) -> String {
    path.file_name()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn validate_cli(cli: &Cli) -> Result<()> {
    if cli.to.as_os_str().is_empty() {
        bail!("--to is required");
    }
    if !(MIN_COMPRESSION_LEVEL..=MAX_COMPRESSION_LEVEL).contains(&cli.compression_level) {
        bail!(
            "--compression-level must be between {MIN_COMPRESSION_LEVEL} and {MAX_COMPRESSION_LEVEL}"
        );
    }
    if cli.jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    if cli.restore.is_some()
        && (!cli.file.is_empty()
            || !cli.dir.is_empty()
            || !cli.regex.is_empty()
            || !cli.roots.is_empty())
    {
        bail!("--restore cannot be combined with backup selectors or search roots");
    }
    Ok(())
}

fn validate_backup_request(request: &BackupRequest) -> Result<()> {
    if request.to.as_os_str().is_empty() {
        bail!("backup destination is required");
    }
    if !(MIN_COMPRESSION_LEVEL..=MAX_COMPRESSION_LEVEL).contains(&request.compression_level) {
        bail!(
            "compression level must be between {MIN_COMPRESSION_LEVEL} and {MAX_COMPRESSION_LEVEL}"
        );
    }
    if request.jobs == 0 {
        bail!("jobs must be at least 1");
    }
    Ok(())
}

fn build_walk_roots(request: &BackupRequest, selectors: &Selectors) -> Result<Vec<WalkRoot>> {
    let roots_were_provided = !request.roots.is_empty();
    let mut needs_implicit_search_root = !request.regexes.is_empty();
    let mut walk_roots = Vec::new();

    for file in &request.files {
        if should_treat_as_direct_path(file, roots_were_provided) {
            let path = PathBuf::from(file);
            if !path.exists() {
                bail!("file selector path does not exist: {}", path.display());
            }
            let meta = fs::symlink_metadata(&path)
                .with_context(|| format!("failed to read metadata for {}", path.display()))?;
            if meta.file_type().is_file() || meta.file_type().is_symlink() {
                walk_roots.push(WalkRoot {
                    start: path,
                    archive_prefix: Some(file_name_or_root(file)?),
                    include_all: true,
                });
            } else {
                bail!(
                    "--file is not a regular file or symlink: {}",
                    path.display()
                );
            }
        } else {
            needs_implicit_search_root = true;
        }
    }

    for dir in &request.dirs {
        if should_treat_as_direct_path(dir, roots_were_provided) {
            let path = PathBuf::from(dir);
            if !path.exists() {
                bail!("directory selector path does not exist: {}", path.display());
            }
            let meta = fs::symlink_metadata(&path)
                .with_context(|| format!("failed to read metadata for {}", path.display()))?;
            if meta.file_type().is_dir() || meta.file_type().is_symlink() {
                walk_roots.push(WalkRoot {
                    start: path,
                    archive_prefix: Some(file_name_or_root(dir)?),
                    include_all: true,
                });
            } else {
                bail!("--dir is not a directory or symlink: {}", path.display());
            }
        } else {
            needs_implicit_search_root = true;
        }
    }

    let mut search_roots = request.roots.clone();
    if search_roots.is_empty()
        && (needs_implicit_search_root
            || (!selectors.has_include_filters() && walk_roots.is_empty()))
    {
        search_roots.push(env::current_dir().context("failed to resolve current directory")?);
    }

    let include_all_for_search_roots = !selectors.has_include_filters();
    for root in search_roots {
        if !root.exists() {
            bail!("search root does not exist: {}", root.display());
        }
        let prefix = if include_all_for_search_roots {
            Some(file_name_or_root_path(&root)?)
        } else {
            None
        };
        walk_roots.push(WalkRoot {
            start: root,
            archive_prefix: prefix,
            include_all: include_all_for_search_roots,
        });
    }

    Ok(deduplicate_walk_roots(walk_roots))
}

fn collect_entries(
    roots: &[WalkRoot],
    selectors: &Selectors,
    no_recursive: bool,
    quiet: bool,
) -> Result<Vec<ScannedEntry>> {
    let mut all_entries: HashMap<String, ScannedEntry> = HashMap::new();
    let mut selected_paths: HashSet<String> = HashSet::new();

    for root in roots {
        let mut selected_dir_prefixes: Vec<String> = Vec::new();
        let max_depth = if no_recursive { 1 } else { usize::MAX };
        let mut stack = vec![(root.start.clone(), 0_usize)];

        while let Some((path, depth)) = stack.pop() {
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    if quiet {
                        return Err(error)
                            .with_context(|| format!("failed to inspect {}", path.display()));
                    }
                    print_padded_stderr(v_concat!(
                        "skip unreadable entry: {}: {error}",
                        path.display()
                    ));
                    continue;
                }
            };

            let file_type = metadata.file_type();
            let kind = if file_type.is_dir() {
                EntryKind::Directory
            } else if file_type.is_file() {
                EntryKind::File
            } else if file_type.is_symlink() {
                EntryKind::Symlink
            } else {
                if !quiet {
                    print_padded_stderr(v_concat!("skip special file {}", path.display()));
                }
                continue;
            };

            let mut descend = kind == EntryKind::Directory && depth < max_depth;

            if let Some(archive_path) =
                archive_path_for(&root.start, root.archive_prefix.as_deref(), &path)
                    .filter(|path| !path.is_empty())
            {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default();
                let excluded_by_directory = if kind == EntryKind::Directory {
                    selectors.excludes_dirish(&archive_path, &path, &name)
                } else {
                    is_under_excluded_directory(selectors, &archive_path, &path)
                };

                let excluded = selectors.excludes_regex(&archive_path, &path)
                    || (kind == EntryKind::File
                        && selectors.excludes_file(&archive_path, &path, &name))
                    || excluded_by_directory;

                if excluded {
                    descend = false;
                } else {
                    let selected_by_ancestor = selected_dir_prefixes
                        .iter()
                        .any(|prefix| path_eq_or_under(&archive_path, prefix));
                    let selected = root.include_all
                        || selected_by_ancestor
                        || match kind {
                            EntryKind::File | EntryKind::Symlink => {
                                selectors.matches_include_file(&archive_path, &path, &name)
                                    || selectors.matches_include_regex(&archive_path, &path)
                            }
                            EntryKind::Directory => {
                                selectors.matches_include_dir(&archive_path, &path, &name)
                                    || selectors.matches_include_regex(&archive_path, &path)
                            }
                        };

                    if selected && kind == EntryKind::Directory {
                        selected_dir_prefixes.push(archive_path.clone());
                    }

                    let meta = metadata_for(&path, archive_path.clone(), kind, &metadata)?;
                    all_entries
                        .entry(archive_path.clone())
                        .or_insert(ScannedEntry {
                            source_path: path.clone(),
                            archive_path: archive_path.clone(),
                            kind,
                            metadata: meta,
                            hash: None,
                        });

                    if selected {
                        selected_paths.insert(archive_path);
                    }
                }
            }

            if descend {
                let mut children = match sorted_child_paths(&path) {
                    Ok(children) => children,
                    Err(error) => {
                        if quiet {
                            return Err(error)
                                .with_context(|| format!("failed to read {}", path.display()));
                        }
                        print_padded_stderr(v_concat!(
                            "skip unreadable directory: {}: {error}",
                            path.display()
                        ));
                        continue;
                    }
                };
                children.reverse();
                stack.extend(children.into_iter().map(|child| (child, depth + 1)));
            }
        }
    }

    let mut ancestor_paths = Vec::new();
    for selected in &selected_paths {
        ancestor_paths.extend(parent_archive_paths(selected));
    }
    for ancestor in ancestor_paths {
        if all_entries.contains_key(&ancestor) {
            selected_paths.insert(ancestor);
        }
    }

    let mut entries: Vec<ScannedEntry> = all_entries
        .into_iter()
        .filter_map(|(path, entry)| selected_paths.contains(&path).then_some(entry))
        .collect();
    entries.sort_by(|a, b| {
        a.archive_path
            .components_count()
            .cmp(&b.archive_path.components_count())
            .then_with(|| a.archive_path.cmp(&b.archive_path))
    });
    Ok(entries)
}

fn sorted_child_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut children = Vec::new();
    for entry in fs::read_dir(path)? {
        children.push(entry?.path());
    }
    children.sort_by(|left, right| {
        let left_name = left.file_name().unwrap_or_default();
        let right_name = right.file_name().unwrap_or_default();
        left_name.cmp(right_name)
    });
    Ok(children)
}

trait ArchivePathOrdering {
    fn components_count(&self) -> usize;
}

impl ArchivePathOrdering for str {
    fn components_count(&self) -> usize {
        self.split('/').filter(|part| !part.is_empty()).count()
    }
}

fn hash_files(entries: &mut [ScannedEntry], jobs: usize) -> Result<()> {
    let tasks: VecDeque<(usize, PathBuf)> = entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| {
            (entry.kind == EntryKind::File).then_some((idx, entry.source_path.clone()))
        })
        .collect();
    if tasks.is_empty() {
        return Ok(());
    }

    let worker_count = jobs.min(tasks.len()).max(1);
    let queue = Arc::new(Mutex::new(tasks));
    let (result_tx, result_rx) = mpsc::channel();
    let mut workers = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let result_tx = result_tx.clone();
        workers.push(thread::spawn(move || {
            loop {
                let task = {
                    let mut queue = queue.lock().expect("hash worker queue lock poisoned");
                    queue.pop_front()
                };
                let Some((idx, path)) = task else {
                    break;
                };
                if result_tx
                    .send(hash_file(&path).map(|hash| (idx, hash)))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    drop(result_tx);

    let mut first_error = None;
    for result in result_rx {
        match result {
            Ok((idx, hash)) => entries[idx].hash = Some(hash),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }

    for worker in workers {
        worker
            .join()
            .map_err(|_| simple_error("hash worker panicked"))?;
    }

    if let Some(error) = first_error {
        return Err(error);
    }

    Ok(())
}

struct Sha256State {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    length_bits: u64,
}

impl Sha256State {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            length_bits: 0,
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        self.length_bits = self
            .length_bits
            .wrapping_add((input.len() as u64).wrapping_mul(8));

        if self.buffer_len > 0 {
            let take = (64 - self.buffer_len).min(input.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&input[..take]);
            self.buffer_len += take;
            input = &input[take..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }

        while input.len() >= 64 {
            let mut block = [0_u8; 64];
            block.copy_from_slice(&input[..64]);
            self.process_block(&block);
            input = &input[64..];
        }

        if !input.is_empty() {
            self.buffer[..input.len()].copy_from_slice(input);
            self.buffer_len = input.len();
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            let block = self.buffer;
            self.process_block(&block);
            self.buffer = [0; 64];
            self.buffer_len = 0;
        }

        self.buffer[self.buffer_len..56].fill(0);
        self.buffer[56..].copy_from_slice(&self.length_bits.to_be_bytes());
        let block = self.buffer;
        self.process_block(&block);

        let mut output = [0_u8; 32];
        for (idx, word) in self.state.iter().enumerate() {
            output[idx * 4..idx * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        output
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];

        let mut words = [0_u32; 64];
        for (idx, chunk) in block.chunks_exact(4).take(16).enumerate() {
            words[idx] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for idx in 16..64 {
            let s0 = words[idx - 15].rotate_right(7)
                ^ words[idx - 15].rotate_right(18)
                ^ (words[idx - 15] >> 3);
            let s1 = words[idx - 2].rotate_right(17)
                ^ words[idx - 2].rotate_right(19)
                ^ (words[idx - 2] >> 10);
            words[idx] = words[idx - 16]
                .wrapping_add(s0)
                .wrapping_add(words[idx - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for idx in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[idx])
                .wrapping_add(words[idx]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256State::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(hex)
}

fn metadata_for(
    _path: &Path,
    archive_path: String,
    kind: EntryKind,
    metadata: &fs::Metadata,
) -> Result<EntryMetadata> {
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    #[cfg(windows)]
    use std::os::windows::fs::MetadataExt;

    Ok(EntryMetadata {
        path: archive_path,
        kind,
        len: if kind == EntryKind::File {
            metadata.len()
        } else {
            0
        },
        readonly: metadata.permissions().readonly(),
        modified: metadata.modified().ok().map(system_time_to_stamp),
        accessed: metadata.accessed().ok().map(system_time_to_stamp),
        created: metadata.created().ok().map(system_time_to_stamp),
        #[cfg(unix)]
        unix: Some(UnixMetadata {
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            rdev: metadata.rdev(),
        }),
        #[cfg(windows)]
        windows: Some(WindowsMetadata {
            file_attributes: metadata.file_attributes(),
        }),
    })
}

fn manifest_to_value(manifest: &ArchiveManifest) -> Vec<u8> {
    let mut payload = Vec::new();
    write_u32(&mut payload, manifest.format_version);
    write_i64(&mut payload, manifest.created_unix_seconds);
    write_string(&mut payload, &manifest.source_os);
    payload
}

fn manifest_from_slice(payload: &[u8]) -> Result<ArchiveManifest> {
    let mut reader = PayloadReader::new(payload);
    let manifest = ArchiveManifest {
        format_version: reader.read_u32()?,
        created_unix_seconds: reader.read_i64()?,
        source_os: reader.read_string()?,
    };
    reader.finish()?;
    Ok(manifest)
}

fn require_archive_source_os(source_os: &Option<String>) -> Result<&str> {
    source_os
        .as_deref()
        .context("archive entry appeared before archive manifest")
}

fn metadata_platform_for_source_os(source_os: &str) -> MetadataPlatform {
    if source_os.eq_ignore_ascii_case("windows") {
        MetadataPlatform::Windows
    } else {
        MetadataPlatform::Unix
    }
}

fn entry_metadata_to_value(meta: &EntryMetadata) -> Vec<u8> {
    let mut payload = Vec::new();
    write_string(&mut payload, &meta.path);
    write_u8(&mut payload, entry_kind_to_byte(meta.kind));
    write_u64(&mut payload, meta.len);
    write_bool(&mut payload, meta.readonly);
    write_file_stamp_option(&mut payload, meta.modified);
    write_file_stamp_option(&mut payload, meta.accessed);
    write_file_stamp_option(&mut payload, meta.created);
    #[cfg(unix)]
    write_unix_metadata_option(&mut payload, meta.unix);
    #[cfg(windows)]
    write_windows_metadata_option(&mut payload, meta.windows);
    payload
}

fn entry_metadata_from_slice_for_os(payload: &[u8], source_os: &str) -> Result<EntryMetadata> {
    let mut reader = PayloadReader::new(payload);
    let meta = entry_metadata_from_reader_for_os(&mut reader, source_os)?;
    reader.finish()?;
    Ok(meta)
}

fn entry_metadata_from_reader_for_os(
    reader: &mut PayloadReader<'_>,
    source_os: &str,
) -> Result<EntryMetadata> {
    let path = reader.read_string()?;
    let kind = entry_kind_from_byte(reader.read_u8()?)?;
    let len = reader.read_u64()?;
    let readonly = reader.read_bool()?;
    let modified = reader.read_file_stamp_option()?;
    let accessed = reader.read_file_stamp_option()?;
    let created = reader.read_file_stamp_option()?;
    let metadata_platform = metadata_platform_for_source_os(source_os);
    #[cfg(unix)]
    let unix = match metadata_platform {
        MetadataPlatform::Unix => {
            reader
                .read_unix_metadata_option_fields()?
                .map(|(mode, uid, gid, rdev)| UnixMetadata {
                    mode,
                    uid,
                    gid,
                    rdev,
                })
        }
        MetadataPlatform::Windows => {
            let _ = reader.read_windows_metadata_option_fields()?;
            None
        }
    };
    #[cfg(windows)]
    let windows = match metadata_platform {
        MetadataPlatform::Windows => reader
            .read_windows_metadata_option_fields()?
            .map(|file_attributes| WindowsMetadata { file_attributes }),
        MetadataPlatform::Unix => {
            let _ = reader.read_unix_metadata_option_fields()?;
            None
        }
    };

    Ok(EntryMetadata {
        path,
        kind,
        len,
        readonly,
        modified,
        accessed,
        created,
        #[cfg(unix)]
        unix,
        #[cfg(windows)]
        windows,
    })
}

fn symlink_record_to_value(record: &SymlinkRecord) -> Vec<u8> {
    let mut payload = entry_metadata_to_value(&record.meta);
    write_string(&mut payload, &record.target);
    write_bool_option(&mut payload, record.target_is_dir);
    payload
}

fn symlink_record_from_slice_for_os(payload: &[u8], source_os: &str) -> Result<SymlinkRecord> {
    let mut reader = PayloadReader::new(payload);
    let record = SymlinkRecord {
        meta: entry_metadata_from_reader_for_os(&mut reader, source_os)?,
        target: reader.read_string()?,
        target_is_dir: reader.read_bool_option()?,
    };
    reader.finish()?;
    Ok(record)
}

fn file_data_record_to_value(record: &FileDataRecord) -> Vec<u8> {
    let mut payload = entry_metadata_to_value(&record.meta);
    write_string(&mut payload, &record.hash);
    write_u64(&mut payload, record.data_len);
    payload
}

fn file_data_record_from_slice_for_os(payload: &[u8], source_os: &str) -> Result<FileDataRecord> {
    let mut reader = PayloadReader::new(payload);
    let record = FileDataRecord {
        meta: entry_metadata_from_reader_for_os(&mut reader, source_os)?,
        hash: reader.read_string()?,
        data_len: reader.read_u64()?,
    };
    reader.finish()?;
    Ok(record)
}

fn file_ref_record_to_value(record: &FileRefRecord) -> Vec<u8> {
    let mut payload = entry_metadata_to_value(&record.meta);
    write_string(&mut payload, &record.hash);
    write_string(&mut payload, &record.original_path);
    payload
}

fn file_ref_record_from_slice_for_os(payload: &[u8], source_os: &str) -> Result<FileRefRecord> {
    let mut reader = PayloadReader::new(payload);
    let record = FileRefRecord {
        meta: entry_metadata_from_reader_for_os(&mut reader, source_os)?,
        hash: reader.read_string()?,
        original_path: reader.read_string()?,
    };
    reader.finish()?;
    Ok(record)
}

fn entry_kind_to_byte(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::File => 1,
        EntryKind::Directory => 2,
        EntryKind::Symlink => 3,
    }
}

fn entry_kind_from_byte(value: u8) -> Result<EntryKind> {
    match value {
        1 => Ok(EntryKind::File),
        2 => Ok(EntryKind::Directory),
        3 => Ok(EntryKind::Symlink),
        _ => bail!("invalid archive entry kind byte {value}"),
    }
}

fn write_u8(payload: &mut Vec<u8>, value: u8) {
    payload.push(value);
}

fn write_bool(payload: &mut Vec<u8>, value: bool) {
    write_u8(payload, u8::from(value));
}

fn write_bool_option(payload: &mut Vec<u8>, value: Option<bool>) {
    match value {
        Some(value) => {
            write_u8(payload, 1);
            write_bool(payload, value);
        }
        None => write_u8(payload, 0),
    }
}

fn write_u32(payload: &mut Vec<u8>, value: u32) {
    payload.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(payload: &mut Vec<u8>, value: u64) {
    payload.extend_from_slice(&value.to_le_bytes());
}

fn write_i64(payload: &mut Vec<u8>, value: i64) {
    payload.extend_from_slice(&value.to_le_bytes());
}

fn write_string(payload: &mut Vec<u8>, value: &str) {
    write_u64(payload, value.len() as u64);
    payload.extend_from_slice(value.as_bytes());
}

fn write_file_stamp_option(payload: &mut Vec<u8>, stamp: Option<FileStamp>) {
    match stamp {
        Some(stamp) => {
            write_u8(payload, 1);
            write_i64(payload, stamp.seconds);
            write_u32(payload, stamp.nanos);
        }
        None => write_u8(payload, 0),
    }
}

#[cfg(unix)]
fn write_unix_metadata_option(payload: &mut Vec<u8>, meta: Option<UnixMetadata>) {
    match meta {
        Some(meta) => {
            write_u8(payload, 1);
            write_u32(payload, meta.mode);
            write_u32(payload, meta.uid);
            write_u32(payload, meta.gid);
            write_u64(payload, meta.rdev);
        }
        None => write_u8(payload, 0),
    }
}

#[cfg(windows)]
fn write_windows_metadata_option(payload: &mut Vec<u8>, meta: Option<WindowsMetadata>) {
    match meta {
        Some(meta) => {
            write_u8(payload, 1);
            write_u32(payload, meta.file_attributes);
        }
        None => write_u8(payload, 0),
    }
}

struct PayloadReader<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl<'a> PayloadReader<'a> {
    fn new(payload: &'a [u8]) -> Self {
        Self { payload, offset: 0 }
    }

    fn finish(&self) -> Result<()> {
        if self.offset == self.payload.len() {
            Ok(())
        } else {
            bail!(
                "archive record has {} trailing byte(s)",
                self.payload.len() - self.offset
            )
        }
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .context("archive record length overflow")?;
        if end > self.payload.len() {
            bail!(
                "archive record ended unexpectedly: needed {} byte(s) at offset {}, payload has {} byte(s)",
                len,
                self.offset,
                self.payload.len()
            );
        }
        let bytes = &self.payload[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_bool(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => bail!("invalid bool byte {value} in archive record"),
        }
    }

    fn read_bool_option(&mut self) -> Result<Option<bool>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_bool()?)),
            value => bail!("invalid optional bool tag {value} in archive record"),
        }
    }

    fn read_u32(&mut self) -> Result<u32> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("slice length checked"),
        ))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_le_bytes(
            bytes.try_into().expect("slice length checked"),
        ))
    }

    fn read_i64(&mut self) -> Result<i64> {
        let bytes = self.read_exact(8)?;
        Ok(i64::from_le_bytes(
            bytes.try_into().expect("slice length checked"),
        ))
    }

    fn read_string(&mut self) -> Result<String> {
        let len = self.read_u64()?;
        if len > usize::MAX as u64 {
            bail!("archive string is too large");
        }
        let bytes = self.read_exact(len as usize)?;
        String::from_utf8(bytes.to_vec()).context("archive string is not valid UTF-8")
    }

    fn read_file_stamp_option(&mut self) -> Result<Option<FileStamp>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(FileStamp {
                seconds: self.read_i64()?,
                nanos: self.read_u32()?,
            })),
            value => bail!("invalid timestamp option tag {value} in archive record"),
        }
    }

    fn read_unix_metadata_option_fields(&mut self) -> Result<Option<(u32, u32, u32, u64)>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some((
                self.read_u32()?,
                self.read_u32()?,
                self.read_u32()?,
                self.read_u64()?,
            ))),
            value => bail!("invalid Unix metadata option tag {value} in archive record"),
        }
    }

    fn read_windows_metadata_option_fields(&mut self) -> Result<Option<u32>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_u32()?)),
            value => bail!("invalid Windows metadata option tag {value} in archive record"),
        }
    }
}

fn write_json_record<W: Write>(writer: &mut W, tag: u8, payload: Vec<u8>) -> Result<()> {
    writer.write_all(&[tag])?;
    writer.write_all(&(payload.len() as u64).to_le_bytes())?;
    writer.write_all(&payload)?;
    Ok(())
}

fn read_json_record<R: Read>(reader: &mut R) -> Result<Option<(u8, Vec<u8>)>> {
    let mut tag = [0_u8; 1];
    match reader.read_exact(&mut tag) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error).context("failed to read archive record tag"),
    }
    let len = match read_u64(reader) {
        Ok(len) => len,
        Err(error) => bail!("failed to read archive record length: {error}"),
    };
    if len > usize::MAX as u64 {
        bail!("archive record is too large to fit in memory");
    }
    let mut json = vec![0_u8; len as usize];
    reader
        .read_exact(&mut json)
        .context("failed to read archive record payload")?;
    Ok(Some((tag[0], json)))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn copy_exact_bytes<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    mut len: u64,
) -> Result<u64> {
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    let mut copied = 0;
    while len > 0 {
        let chunk_len = buffer.len().min(len as usize);
        let read = reader
            .read(&mut buffer[..chunk_len])
            .context("failed while reading data stream")?;
        if read == 0 {
            bail!("unexpected end of stream while copying file data");
        }
        writer
            .write_all(&buffer[..read])
            .context("failed while writing data stream")?;
        len -= read as u64;
        copied += read as u64;
    }
    Ok(copied)
}

fn resolve_backup_output_path(to: &Path) -> Result<PathBuf> {
    let text = to.to_string_lossy();
    let looks_like_directory = text.ends_with('/') || text.ends_with('\\') || to.is_dir();
    if looks_like_directory {
        fs::create_dir_all(to)
            .with_context(|| format!("failed to create archive directory {}", to.display()))?;
        Ok(to.join(format!("v_fs_backup-{}.fsb", now_unix_seconds())))
    } else {
        Ok(ensure_fsb_extension(to))
    }
}

fn ensure_fsb_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(ARCHIVE_EXTENSION))
    {
        return path.to_path_buf();
    }

    let mut path = path.as_os_str().to_os_string();
    path.push(".fsb");
    PathBuf::from(path)
}

fn safe_restore_path(root: &Path, archive_path: &str) -> Result<PathBuf> {
    let archive_path = archive_path.replace('\\', "/");
    if archive_path.is_empty()
        || archive_path.starts_with('/')
        || archive_path.contains('\0')
        || archive_path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("unsafe archive path: {archive_path:?}");
    }
    Ok(archive_path
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part)))
}

fn create_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    }
    Ok(())
}

fn prepare_restore_target(path: &Path, overwrite: bool) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !overwrite {
                bail!("restore target already exists: {}", path.display());
            }
            let file_type = metadata.file_type();
            if file_type.is_dir() && !file_type.is_symlink() {
                bail!(
                    "restore target is an existing directory and will not be replaced: {}",
                    path.display()
                );
            }
            fs::remove_file(path)
                .with_context(|| format!("failed to remove existing {}", path.display()))?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect {}", path.display()));
        }
    }
    Ok(())
}

fn apply_metadata(path: &Path, meta: &EntryMetadata) -> Result<()> {
    #[cfg(unix)]
    {
        if let Some(unix) = meta.unix {
            try_chown(path, unix.uid, unix.gid)?;
        }
    }

    if let (Some(accessed), Some(modified)) = (meta.accessed, meta.modified) {
        let times = fs::FileTimes::new()
            .set_accessed(stamp_to_system_time(accessed))
            .set_modified(stamp_to_system_time(modified));
        if let Ok(file) = OpenOptions::new().read(true).write(true).open(path) {
            file.set_times(times)
                .with_context(|| format!("failed to restore timestamps for {}", path.display()))?;
        }
    }

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to read restored metadata for {}", path.display()))?
        .permissions();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(unix) = meta.unix {
            permissions.set_mode(unix.mode);
        }
    }
    permissions.set_readonly(meta.readonly);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to restore permissions for {}", path.display()))?;

    Ok(())
}

#[cfg(unix)]
fn try_chown(path: &Path, uid: u32, gid: u32) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("path contains interior NUL: {}", path.display()))?;
    let result = unsafe { libc::chown(c_path.as_ptr(), uid, gid) };
    if result == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    let ownership_restore_is_best_effort = error.kind() == io::ErrorKind::PermissionDenied
        || matches!(error.raw_os_error(), Some(code)
            if code == libc::EINVAL
                || code == libc::ENOTSUP
                || code == libc::EOPNOTSUPP
                || code == libc::ENOSYS);
    if ownership_restore_is_best_effort {
        return Ok(());
    }
    Err(error).with_context(|| format!("failed to restore owner for {}", path.display()))
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path, _target_is_dir: Option<bool>) -> Result<()> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path, target_is_dir: Option<bool>) -> Result<()> {
    if target_is_dir.unwrap_or(false) {
        std::os::windows::fs::symlink_dir(target, link)?;
    } else {
        std::os::windows::fs::symlink_file(target, link)?;
    }
    Ok(())
}

fn compile_regexes(values: &[String]) -> Result<Vec<Regex>> {
    values
        .iter()
        .map(|value| compile_user_regex(value))
        .collect()
}

fn compile_user_regex(input: &str) -> Result<Regex> {
    if let Some((pattern, flags)) = parse_slash_regex(input) {
        let mut builder = RegexBuilder::new(pattern);
        for flag in flags.chars() {
            match flag {
                'g' => {}
                'i' => {
                    builder.case_insensitive(true);
                }
                'm' => {
                    builder.multi_line(true);
                }
                's' => {
                    builder.dot_matches_new_line(true);
                }
                'x' => {
                    builder.ignore_whitespace(true);
                }
                'U' => {
                    builder.swap_greed(true);
                }
                other => bail!("unsupported regex flag {other:?} in {input:?}"),
            }
        }
        Ok(builder.build())
    } else {
        Ok(Regex::new(input))
    }
}

fn regex_extension_hint(pattern: &str) -> Option<String> {
    let marker = pattern.rfind(r"\.")?;
    let mut extension = String::from(".");
    for ch in pattern[marker + 2..].chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            extension.push(ch);
        } else {
            break;
        }
    }
    (extension.len() > 1).then_some(extension)
}

fn regex_literal_hint(pattern: &str) -> String {
    let mut literal = String::new();
    let mut escaped = false;
    for ch in pattern.chars() {
        if escaped {
            literal.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '+' | '|') {
            continue;
        }
        literal.push(ch);
    }
    literal
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v) = (0, 0);
    let mut star = None;
    let mut star_value = 0;

    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            star_value = v;
        } else if let Some(star_idx) = star {
            p = star_idx + 1;
            star_value += 1;
            v = star_value;
        } else {
            return false;
        }
    }

    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn parse_slash_regex(input: &str) -> Option<(&str, &str)> {
    if !input.starts_with('/') {
        return None;
    }
    let mut escaped = false;
    let mut last_slash = None;
    for (idx, ch) in input.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '/' {
            last_slash = Some(idx);
        }
    }
    let idx = last_slash?;
    Some((&input[1..idx], &input[idx + 1..]))
}

fn archive_path_for(start: &Path, prefix: Option<&str>, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(start).ok()?;
    let rel = normalize_archive_path(rel);
    match (prefix, rel.is_empty()) {
        (Some(prefix), true) => Some(prefix.to_string()),
        (Some(prefix), false) => Some(format!("{prefix}/{rel}")),
        (None, true) => None,
        (None, false) => Some(rel),
    }
}

fn normalize_archive_path(path: &Path) -> String {
    normalize_path_lossy(path)
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string()
}

fn normalize_path_lossy(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_match_text(value: &str) -> String {
    let mut normalized = value.replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_string();
    }
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    #[cfg(windows)]
    {
        normalized = normalized.to_lowercase();
    }
    normalized
}

fn path_eq_or_under(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn path_eq_or_nested_match(path: &str, pattern: &str) -> bool {
    path == pattern
        || path_eq_or_under(path, pattern)
        || path
            .strip_suffix(pattern)
            .is_some_and(|prefix| prefix.ends_with('/'))
}

fn path_contains_component_path(path: &str, pattern: &str) -> bool {
    path == pattern
        || path.starts_with(&format!("{pattern}/"))
        || path.ends_with(&format!("/{pattern}"))
        || path.contains(&format!("/{pattern}/"))
}

fn parent_archive_paths(path: &str) -> Vec<String> {
    let mut parents = Vec::new();
    let mut current = path;
    while let Some((parent, _)) = current.rsplit_once('/') {
        if parent.is_empty() {
            break;
        }
        parents.push(parent.to_string());
        current = parent;
    }
    parents
}

fn is_under_excluded_directory(
    selectors: &Selectors,
    archive_path: &str,
    absolute_path: &Path,
) -> bool {
    let Some((parent_archive_path, _)) = archive_path.rsplit_once('/') else {
        return false;
    };
    let parent_absolute_path = absolute_path.parent().unwrap_or(absolute_path);
    let parent_name = parent_absolute_path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    selectors.excludes_dirish(parent_archive_path, parent_absolute_path, &parent_name)
}

fn should_treat_as_direct_path(raw: &str, roots_were_provided: bool) -> bool {
    let path = Path::new(raw);
    path.is_absolute()
        || raw.contains('/')
        || raw.contains('\\')
        || (!roots_were_provided && path.exists())
}

fn file_name_or_root(raw: &str) -> Result<String> {
    file_name_or_root_path(Path::new(raw))
}

fn file_name_or_root_path(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .or_else(|| {
            path.components()
                .next_back()
                .map(|component| component.as_os_str())
        })
        .context("could not determine archive name for path")?;
    let value = normalize_archive_path(Path::new(name));
    if value.is_empty() {
        bail!("could not determine archive name for {}", path.display());
    }
    Ok(value)
}

fn deduplicate_walk_roots(roots: Vec<WalkRoot>) -> Vec<WalkRoot> {
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter(|root| {
            let key = format!(
                "{}\0{}\0{}",
                normalize_path_lossy(&root.start),
                root.archive_prefix.clone().unwrap_or_default(),
                root.include_all
            );
            seen.insert(key)
        })
        .collect()
}

fn system_time_to_stamp(time: SystemTime) -> FileStamp {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => FileStamp {
            seconds: duration.as_secs() as i64,
            nanos: duration.subsec_nanos(),
        },
        Err(error) => {
            let duration = error.duration();
            FileStamp {
                seconds: -(duration.as_secs() as i64),
                nanos: duration.subsec_nanos(),
            }
        }
    }
}

fn stamp_to_system_time(stamp: FileStamp) -> SystemTime {
    if stamp.seconds >= 0 {
        UNIX_EPOCH + Duration::new(stamp.seconds as u64, stamp.nanos)
    } else {
        UNIX_EPOCH - Duration::new(stamp.seconds.unsigned_abs(), stamp.nanos)
    }
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn color(code: &str, text: impl AsRef<str>) -> String {
    format!("{code}{}{ANSI_RESET}", text.as_ref())
}

fn fact_line(label: &str, value: impl AsRef<str>) -> String {
    let label = format!("{label:<30}:");
    format!("  {} {}", color(ANSI_CYAN, label), color(ANSI_GREEN, value))
}

fn print_backup_summary(stats: &BackupStats, elapsed: Duration) {
    let mut output = color(ANSI_GREEN, "Backup complete");
    output.push('\n');
    output.push_str(&fact_line("Entries", stats.entries.to_string()));
    output.push('\n');
    output.push_str(&fact_line("Input", human_bytes(stats.original_bytes)));
    output.push('\n');
    output.push_str(&fact_line(
        "Stored before compression",
        human_bytes(stats.stored_file_bytes),
    ));
    output.push('\n');
    output.push_str(&fact_line("Archive size", human_bytes(stats.archive_bytes)));
    if stats.deduplicated_bytes > 0 {
        output.push('\n');
        output.push_str(&fact_line(
            "Deduplicated",
            format!(
                "{} of duplicate file content",
                human_bytes(stats.deduplicated_bytes)
            ),
        ));
    }
    output.push('\n');
    output.push_str(&fact_line(
        "Archive",
        stats.archive_path.display().to_string(),
    ));
    output.push('\n');
    output.push_str(&fact_line("Total time", human_duration(elapsed)));
    print_padded_stderr(output);
}

fn print_restore_summary(stats: &RestoreStats, elapsed: Duration) {
    let mut output = color(ANSI_GREEN, "Restore complete");
    output.push('\n');
    output.push_str(&fact_line("Entries", stats.entries.to_string()));
    output.push('\n');
    output.push_str(&fact_line("Restored", human_bytes(stats.restored_bytes)));
    output.push('\n');
    output.push_str(&fact_line("Total time", human_duration(elapsed)));
    print_padded_stderr(output);
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.2} {}", UNITS[unit])
    }
}

fn print_sized_progress(label: &str, bytes: u64, path: &str, note: Option<&str>) {
    let size = format!("{:>12}", human_bytes(bytes));
    let suffix = note
        .map(|value| format!(" {}", color(ANSI_YELLOW, format!("({value})"))))
        .unwrap_or_default();
    print_padded_stderr(v_concat!(
        "{} ({}) {}{}",
        label,
        color(ANSI_GREEN, size),
        path,
        suffix
    ));
}

fn print_time_row(label: &str, duration: Duration) {
    print_padded_stderr(color(
        ANSI_YELLOW,
        format!("{:<14} {}", label, human_duration(duration)),
    ));
}

fn human_duration(duration: Duration) -> String {
    if duration.as_secs() == 0 {
        return format!("{}ms", duration.as_millis());
    }

    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().saturating_sub(1).max(1))
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn tempdir() -> io::Result<TestDir> {
        let mut path = env::temp_dir();
        path.push(format!(
            "v_fs_backup-test-{}-{}",
            std::process::id(),
            now_unix_seconds()
        ));
        let mut attempt = 0_u32;
        loop {
            let candidate = if attempt == 0 {
                path.clone()
            } else {
                PathBuf::from(format!("{}-{attempt}", path.display()))
            };
            match fs::create_dir(&candidate) {
                Ok(()) => return Ok(TestDir { path: candidate }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn foreign_source_os() -> &'static str {
        if cfg!(windows) { "linux" } else { "windows" }
    }

    fn write_foreign_platform_metadata_option(payload: &mut Vec<u8>) {
        if cfg!(windows) {
            write_u8(payload, 1);
            write_u32(payload, 0o100644);
            write_u32(payload, 1000);
            write_u32(payload, 1000);
            write_u64(payload, 0);
        } else {
            write_u8(payload, 1);
            write_u32(payload, 0x20);
        }
    }

    fn foreign_file_data_record_payload(path: &str, data_len: u64, hash: &str) -> Vec<u8> {
        let mut payload = Vec::new();
        write_string(&mut payload, path);
        write_u8(&mut payload, entry_kind_to_byte(EntryKind::File));
        write_u64(&mut payload, data_len);
        write_bool(&mut payload, false);
        write_file_stamp_option(&mut payload, None);
        write_file_stamp_option(&mut payload, None);
        write_file_stamp_option(&mut payload, None);
        write_foreign_platform_metadata_option(&mut payload);
        write_string(&mut payload, hash);
        write_u64(&mut payload, data_len);
        payload
    }

    #[test]
    fn backup_output_path_adds_fsb_when_missing() {
        assert_eq!(
            resolve_backup_output_path(Path::new("backup")).unwrap(),
            PathBuf::from("backup.fsb")
        );
        assert_eq!(
            resolve_backup_output_path(Path::new("backup.FSB")).unwrap(),
            PathBuf::from("backup.FSB")
        );
    }

    #[test]
    fn backup_output_directory_uses_generated_fsb_file() {
        let tmp = tempdir().unwrap();
        let output = resolve_backup_output_path(tmp.path()).unwrap();

        assert_eq!(output.parent(), Some(tmp.path()));
        assert_eq!(
            output.extension().and_then(|value| value.to_str()),
            Some("fsb")
        );
    }

    #[test]
    fn parses_requested_multi_letter_aliases() {
        let args = normalize_cli_args([
            OsString::from("v_fs_backup"),
            OsString::from("-nr"),
            OsString::from("-rx=/\\.png$/i"),
            OsString::from("-ed"),
            OsString::from(".git"),
            OsString::from("--to"),
            OsString::from("backup.fsb"),
        ]);
        let cli = Cli::parse_from(args);
        assert!(cli.no_recursive);
        assert_eq!(cli.regex, vec!["/\\.png$/i"]);
        assert_eq!(cli.exclude_dir, vec![".git"]);
    }

    #[test]
    fn interactive_line_splits_quoted_paths() {
        let words =
            split_interactive_line(r#"compress "C:\Users\A B" "D:\Backups\one.fsb""#).unwrap();

        assert_eq!(
            words,
            vec![
                "compress".to_string(),
                r#"C:\Users\A B"#.to_string(),
                r#"D:\Backups\one.fsb"#.to_string(),
            ]
        );
    }

    #[test]
    fn interactive_compress_shortcut_maps_to_cli_args() {
        let args =
            interactive_args_from_line(r#"compress "/data/src" "/backup/data.fsb""#).unwrap();
        let cli = Cli::parse_from(args);

        assert_eq!(cli.dir, vec!["/data/src"]);
        assert_eq!(cli.to, PathBuf::from("/backup/data.fsb"));
    }

    #[test]
    fn interactive_decompress_shortcut_maps_to_restore_args() {
        let args = interactive_args_from_line(r#"decompress "backup.fsb" "restore dir""#).unwrap();
        let cli = Cli::parse_from(args);

        assert_eq!(cli.restore, Some(PathBuf::from("backup.fsb")));
        assert_eq!(cli.to, PathBuf::from("restore dir"));
    }

    #[test]
    fn interactive_command_completion_includes_commands_and_flags() {
        let commands = command_completion_pairs("com");
        assert!(commands.iter().any(|pair| pair.replacement == "compress "));

        let flags = command_completion_pairs("--to");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].replacement, "--to ");
    }

    #[test]
    fn interactive_path_completion_quotes_paths_with_spaces() {
        let tmp = tempdir().unwrap();
        let spaced = tmp.path().join("Program Files");
        fs::create_dir(&spaced).unwrap();

        let prefix = tmp.path().join("Program").display().to_string();
        let token = CompletionToken {
            start: 0,
            unquoted: prefix,
            quote: None,
        };
        let pairs = path_completion_pairs(&token);
        let expected_display = format!("{}{}", spaced.display(), std::path::MAIN_SEPARATOR);
        let expected_replacement = format!("\"{expected_display}\"");

        assert!(pairs.iter().any(|pair| {
            pair.display == expected_display && pair.replacement == expected_replacement
        }));
        assert_eq!(
            quote_path_completion(r"C:\Program Files", None),
            r#""C:\Program Files""#
        );
    }

    #[test]
    fn accepts_legacy_compression_spelling_and_level_22() {
        let args = normalize_cli_args([
            OsString::from("v_fs_backup"),
            OsString::from("--compresion-level=22"),
            OsString::from("--to"),
            OsString::from("backup.fsb"),
            OsString::from("source"),
        ]);
        let cli = Cli::parse_from(args);

        assert_eq!(cli.compression_level, 22);
        validate_cli(&cli).unwrap();
    }

    #[test]
    fn slash_regex_flags_are_supported() {
        let rx = compile_user_regex("/photo\\.png/im").unwrap();
        assert!(rx.is_match("PHOTO.png"));
    }

    #[test]
    fn local_sha256_matches_known_vector() {
        let mut hasher = Sha256State::new();
        hasher.update(b"abc");
        let digest = hasher.finalize();
        let mut hex = String::new();
        for byte in digest {
            use std::fmt::Write as _;
            write!(&mut hex, "{byte:02x}").unwrap();
        }
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn backup_restore_roundtrip_includes_hidden_and_dedupes() {
        let tmp = tempdir().unwrap();
        let source = tmp.path().join("source");
        let hidden = source.join(".git");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(source.join("a.txt"), b"same content").unwrap();
        fs::write(source.join("b.txt"), b"same content").unwrap();
        fs::write(hidden.join("config"), b"[core]\n").unwrap();

        let archive = tmp.path().join("backup.fsb");
        let stats = create_backup(BackupRequest {
            roots: vec![source.clone()],
            files: vec![],
            dirs: vec![],
            regexes: vec![],
            exclude_files: vec![],
            exclude_dirs: vec![],
            exclude_regexes: vec![],
            no_recursive: false,
            to: archive.clone(),
            compression_level: 3,
            jobs: 1,
            overwrite: false,
            quiet: true,
        })
        .unwrap();
        assert!(stats.deduplicated_bytes > 0);

        let restore_to = tmp.path().join("restore");
        restore_archive(RestoreRequest {
            archive,
            to: restore_to.clone(),
            overwrite: false,
            quiet: true,
        })
        .unwrap();

        assert_eq!(
            fs::read(restore_to.join("source").join(".git").join("config")).unwrap(),
            b"[core]\n"
        );
        assert_eq!(
            fs::read(restore_to.join("source").join("b.txt")).unwrap(),
            b"same content"
        );
    }

    #[test]
    fn restores_archive_with_foreign_platform_metadata() {
        let tmp = tempdir().unwrap();
        let archive = tmp.path().join("foreign.fsb");
        let data = b"foreign metadata";
        let file = File::create(&archive).unwrap();
        let mut writer = BufWriter::new(file);
        writer.write_all(MAGIC).unwrap();
        let mut encoder = zstd::stream::write::Encoder::new(writer, 1).unwrap();

        write_json_record(
            &mut encoder,
            TAG_MANIFEST,
            manifest_to_value(&ArchiveManifest {
                format_version: FORMAT_VERSION,
                created_unix_seconds: 0,
                source_os: foreign_source_os().to_string(),
            }),
        )
        .unwrap();
        write_json_record(
            &mut encoder,
            TAG_FILE_DATA,
            foreign_file_data_record_payload("foreign.txt", data.len() as u64, "foreign-hash"),
        )
        .unwrap();
        encoder.write_all(data).unwrap();
        let mut writer = encoder.finish().unwrap();
        writer.flush().unwrap();

        let restore_to = tmp.path().join("restore");
        let stats = restore_archive(RestoreRequest {
            archive,
            to: restore_to.clone(),
            overwrite: false,
            quiet: true,
        })
        .unwrap();

        assert_eq!(stats.files, 1);
        assert_eq!(fs::read(restore_to.join("foreign.txt")).unwrap(), data);
    }

    #[test]
    fn restores_legacy_rle_archive() {
        let tmp = tempdir().unwrap();
        let archive = tmp.path().join("legacy.fsb");
        let file = File::create(&archive).unwrap();
        let mut writer = BufWriter::new(file);
        writer.write_all(LEGACY_RLE_MAGIC).unwrap();
        let mut encoder = RleEncoder::new(writer, 6);
        let data = b"legacy rle";
        let meta = EntryMetadata {
            path: "legacy.txt".to_string(),
            kind: EntryKind::File,
            len: data.len() as u64,
            readonly: false,
            modified: None,
            accessed: None,
            created: None,
            #[cfg(unix)]
            unix: None,
            #[cfg(windows)]
            windows: None,
        };

        write_json_record(
            &mut encoder,
            TAG_MANIFEST,
            manifest_to_value(&ArchiveManifest {
                format_version: LEGACY_FORMAT_VERSION,
                created_unix_seconds: 0,
                source_os: "test".to_string(),
            }),
        )
        .unwrap();
        write_json_record(
            &mut encoder,
            TAG_FILE_DATA,
            file_data_record_to_value(&FileDataRecord {
                meta,
                hash: "legacy-hash".to_string(),
                data_len: data.len() as u64,
            }),
        )
        .unwrap();
        encoder.write_all(data).unwrap();
        let mut writer = encoder.finish().unwrap();
        writer.flush().unwrap();

        let restore_to = tmp.path().join("restore");
        let stats = restore_archive(RestoreRequest {
            archive,
            to: restore_to.clone(),
            overwrite: false,
            quiet: true,
        })
        .unwrap();

        assert_eq!(stats.files, 1);
        assert_eq!(fs::read(restore_to.join("legacy.txt")).unwrap(), data);
    }

    #[test]
    fn excludes_directories_before_selection() {
        let tmp = tempdir().unwrap();
        let source = tmp.path().join("source");
        fs::create_dir_all(source.join("keep")).unwrap();
        fs::create_dir_all(source.join("skip")).unwrap();
        fs::write(source.join("keep").join("a.txt"), b"a").unwrap();
        fs::write(source.join("skip").join("b.txt"), b"b").unwrap();

        let archive = tmp.path().join("backup.fsb");
        create_backup(BackupRequest {
            roots: vec![source.clone()],
            files: vec![],
            dirs: vec![],
            regexes: vec![],
            exclude_files: vec![],
            exclude_dirs: vec!["skip".to_string()],
            exclude_regexes: vec![],
            no_recursive: false,
            to: archive.clone(),
            compression_level: 1,
            jobs: 1,
            overwrite: false,
            quiet: true,
        })
        .unwrap();

        let restore_to = tmp.path().join("restore");
        restore_archive(RestoreRequest {
            archive,
            to: restore_to.clone(),
            overwrite: false,
            quiet: true,
        })
        .unwrap();

        assert!(
            restore_to
                .join("source")
                .join("keep")
                .join("a.txt")
                .exists()
        );
        assert!(!restore_to.join("source").join("skip").exists());
    }

    #[test]
    fn absolute_dir_selector_does_not_add_current_dir_search_root() {
        let tmp = tempdir().unwrap();
        let source = tmp.path().join("source");
        fs::create_dir_all(&source).unwrap();

        let request = BackupRequest {
            roots: vec![],
            files: vec![],
            dirs: vec![source.to_string_lossy().to_string()],
            regexes: vec![],
            exclude_files: vec![],
            exclude_dirs: vec![],
            exclude_regexes: vec![],
            no_recursive: false,
            to: tmp.path().join("backup.fsb"),
            compression_level: 1,
            jobs: 1,
            overwrite: false,
            quiet: true,
        };

        let selectors = Selectors::new(&request).unwrap();
        let roots = build_walk_roots(&request, &selectors).unwrap();

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].start, source);
        assert!(roots[0].include_all);
    }

    #[test]
    fn missing_direct_path_selector_fails_instead_of_searching_cwd() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("missing");
        let request = BackupRequest {
            roots: vec![],
            files: vec![],
            dirs: vec![missing.to_string_lossy().to_string()],
            regexes: vec![],
            exclude_files: vec![],
            exclude_dirs: vec![],
            exclude_regexes: vec![],
            no_recursive: false,
            to: tmp.path().join("backup.fsb"),
            compression_level: 1,
            jobs: 1,
            overwrite: false,
            quiet: true,
        };

        let selectors = Selectors::new(&request).unwrap();
        let error = build_walk_roots(&request, &selectors).unwrap_err();

        assert!(error.to_string().contains("does not exist"));
    }
}
