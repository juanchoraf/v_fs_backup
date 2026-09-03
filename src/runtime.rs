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
    {
        windows_terminal::enable_ansi_colors();
        if let Ok(executable) = env::current_exe() {
            windows_terminal::apply_console_icon(&executable);
        }
    }
}
