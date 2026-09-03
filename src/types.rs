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
enum ParsedCommand {
    Run(Cli),
    CheckUpdate,
    Update,
    Install,
    Uninstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlMode {
    CheckUpdate,
    Update,
    InstallApp,
    UninstallApp,
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
