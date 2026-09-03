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

