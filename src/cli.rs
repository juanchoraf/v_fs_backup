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
        match parse_command_from(args)? {
            Some(ParsedCommand::Run(cli)) => Ok(Some(cli)),
            Some(ParsedCommand::CheckUpdate | ParsedCommand::Update) => {
                bail!("update commands do not produce backup arguments")
            }
            None => Ok(None),
        }
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

fn parse_command_from(args: impl IntoIterator<Item = OsString>) -> Result<Option<ParsedCommand>> {
    let args = normalize_cli_args(args);
    let mut cli = Cli::default();
    let mut idx = 1;
    let mut update_mode = None;

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
        if text == "--check-update" {
            set_update_mode(&mut update_mode, UpdateMode::Check)?;
            idx += 1;
            continue;
        }
        if text == "--update" {
            set_update_mode(&mut update_mode, UpdateMode::Install)?;
            idx += 1;
            continue;
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

        if let Some((flag, value)) = text.split_once('=') {
            if flag == "--check-update" || flag == "--update" {
                if !value.is_empty() {
                    bail!("{flag} does not take a value");
                }
                let mode = if flag == "--check-update" {
                    UpdateMode::Check
                } else {
                    UpdateMode::Install
                };
                set_update_mode(&mut update_mode, mode)?;
                idx += 1;
                continue;
            }
        }

        if text.starts_with('-') {
            bail!("unknown option {text}");
        }

        cli.roots.push(PathBuf::from(arg));
        idx += 1;
    }

    if let Some(update_mode) = update_mode {
        if cli_has_backup_or_restore_input(&cli) {
            bail!("--check-update and --update cannot be combined with backup or restore options");
        }
        return Ok(Some(match update_mode {
            UpdateMode::Check => ParsedCommand::CheckUpdate,
            UpdateMode::Install => ParsedCommand::Update,
        }));
    }

    Ok(Some(ParsedCommand::Run(cli)))
}

fn set_update_mode(current: &mut Option<UpdateMode>, mode: UpdateMode) -> Result<()> {
    if current.is_some() {
        bail!("only one update option is allowed per run");
    }
    *current = Some(mode);
    Ok(())
}

fn cli_has_backup_or_restore_input(cli: &Cli) -> bool {
    !cli.file.is_empty()
        || !cli.dir.is_empty()
        || !cli.regex.is_empty()
        || !cli.exclude_file.is_empty()
        || !cli.exclude_dir.is_empty()
        || !cli.exclude_regex.is_empty()
        || cli.no_recursive
        || !cli.to.as_os_str().is_empty()
        || cli.restore.is_some()
        || cli.compression_level != 6
        || cli.jobs != default_jobs()
        || cli.overwrite
        || cli.quiet
        || !cli.roots.is_empty()
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
    let version = env!("CARGO_PKG_VERSION");
    print_padded_stdout(v_concat!(
        "v_fs_backup {version}\n\nFast, compressed, metadata-preserving filesystem backups.\n\nUSAGE:\n  v_fs_backup [OPTIONS] [SEARCH_ROOT ...] --to <ARCHIVE_OR_DIRECTORY>\n  v_fs_backup --restore <ARCHIVE> --to <RESTORE_DIRECTORY>\n  v_fs_backup --check-update\n  v_fs_backup --update\n\nOPTIONS:\n  --file <PATH_OR_NAME>             Back up a matching file\n  --dir <PATH_OR_NAME>              Back up a matching directory\n  --regex, --rx <REGEX>             Back up paths matching a regex\n  --exclude-file, --ef <PATH>       Exclude a file\n  --exclude-dir, --ed <PATH>        Exclude a directory tree\n  --exclude-regex, --er <REGEX>     Exclude paths matching a regex\n  -n, --no-recursive                Do not recurse into subdirectories\n  --to <PATH>                       Archive path for backups or restore directory\n  --restore <ARCHIVE>               Restore a v_fs_backup archive\n  --compression-level <0..22>       zstd compression level, default 6\n  --jobs <N>                        Hashing worker count\n  --overwrite                       Replace an existing archive or restore target\n  --quiet                           Suppress progress output\n  --check-update                    Check GitHub for a newer release\n  --update                          Install the latest matching GitHub release\n  -h, --help                        Show this help\n  -V, --version                     Show version\n\nUpdates are pulled from https://github.com/juanchoraf/v_fs_backup/releases/latest.\nCompatibility aliases accepted before parsing: -nr, -rx, -ef, -ed, -er, --compresion-level."
    ));
}
