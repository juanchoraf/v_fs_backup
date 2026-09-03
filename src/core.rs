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
