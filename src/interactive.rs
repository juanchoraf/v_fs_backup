fn run_interactive_shell() -> Result<()> {
    print_interactive_banner();
    let theme = interactive_theme();

    loop {
        let action = select_navigation(
            &theme,
            "Choose an action",
            &[
                "Create a backup",
                "Restore a backup",
                "Check for updates",
                "Update v_fs_backup",
                "Clear screen",
                "Quit",
            ],
            0,
        )?;

        match action {
            Navigation::Back => continue,
            Navigation::Quit | Navigation::Value(5) => {
                v_concat_println!("  Goodbye.\n");
                return Ok(());
            }
            Navigation::Value(0) => {
                if keep_interactive_open_on_error(create_backup_flow(&theme)) {
                    return Ok(());
                }
            }
            Navigation::Value(1) => {
                if keep_interactive_open_on_error(restore_backup_flow(&theme)) {
                    return Ok(());
                }
            }
            Navigation::Value(2) => keep_simple_flow_open(check_update_flow()),
            Navigation::Value(3) => keep_simple_flow_open(update_flow()),
            Navigation::Value(4) => clear_interactive_screen()?,
            Navigation::Value(_) => continue,
        }
    }
}

fn keep_interactive_open_on_error(result: Result<Navigation<()>>) -> bool {
    match result {
        Ok(Navigation::Quit) => true,
        Ok(Navigation::Value(()) | Navigation::Back) => false,
        Err(error) => {
            print_error(&*error);
            wait_for_interactive_enter();
            false
        }
    }
}

fn keep_simple_flow_open(result: Result<()>) {
    if let Err(error) = result {
        print_error(&*error);
        wait_for_interactive_enter();
    }
}

#[derive(Debug, Clone, Copy)]
enum BackupSourceKind {
    File,
    Folder,
}

#[derive(Debug, Clone)]
struct BackupSource {
    kind: BackupSourceKind,
    path: PathBuf,
}

fn create_backup_flow(theme: &InteractiveTheme) -> Result<Navigation<()>> {
    let mut cli = Cli::default();
    let mut source: Option<BackupSource> = None;
    let mut source_label: Option<String> = None;
    let mut step = 0_u8;

    loop {
        match step {
            0 => match choose_backup_source(theme)? {
                Navigation::Value(selected) => {
                    cli = Cli::default();
                    let path = selected.path.display().to_string();
                    match selected.kind {
                        BackupSourceKind::File => cli.file.push(path.clone()),
                        BackupSourceKind::Folder => cli.dir.push(path.clone()),
                    }
                    source_label = Some(path);
                    source = Some(selected);
                    step = 1;
                }
                Navigation::Back => return Ok(Navigation::Back),
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            1 => {
                let selected = source.as_ref().expect("source selected before destination");
                let default_archive = default_archive_path(&selected.path);
                match read_path_navigation(
                    "Where do you want to save the backup?",
                    Some(&default_archive),
                )? {
                    Navigation::Value(path) => {
                        cli.to = path;
                        step = 2;
                    }
                    Navigation::Back => step = 0,
                    Navigation::Quit => return Ok(Navigation::Quit),
                }
            }
            2 => match configure_backup_options(theme, &mut cli)? {
                Navigation::Value(()) => {
                    cli.exclude_file.clear();
                    cli.exclude_dir.clear();
                    cli.exclude_regex.clear();
                    step = 3;
                }
                Navigation::Back => step = 1,
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            3 => match configure_exclusions(theme, &mut cli)? {
                Navigation::Value(()) => step = 4,
                Navigation::Back => step = 2,
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            _ => {
                print_backup_confirmation(
                    &cli,
                    source_label
                        .as_deref()
                        .expect("source selected before confirmation"),
                );
                match select_navigation(
                    theme,
                    "Ready to create the backup",
                    &["Start backup", "Back", "Quit"],
                    0,
                )? {
                    Navigation::Value(0) => {
                        run(cli)?;
                        return Ok(Navigation::Value(()));
                    }
                    Navigation::Value(1) | Navigation::Back => step = 3,
                    Navigation::Value(_) | Navigation::Quit => return Ok(Navigation::Quit),
                }
            }
        }
    }
}

fn choose_backup_source(theme: &InteractiveTheme) -> Result<Navigation<BackupSource>> {
    loop {
        let kind = match select_navigation(
            theme,
            "What do you want to back up?",
            &["A File", "A Folder", "Back", "Quit"],
            0,
        )? {
            Navigation::Value(0) => BackupSourceKind::File,
            Navigation::Value(1) => BackupSourceKind::Folder,
            Navigation::Value(2) | Navigation::Back => return Ok(Navigation::Back),
            Navigation::Value(_) | Navigation::Quit => return Ok(Navigation::Quit),
        };
        let label = match kind {
            BackupSourceKind::File => "Type or paste the file full path",
            BackupSourceKind::Folder => "Type or paste the folder full path",
        };
        let path = match read_path_navigation(label, None)? {
            Navigation::Value(path) => path,
            Navigation::Back => continue,
            Navigation::Quit => return Ok(Navigation::Quit),
        };
        if backup_source_is_valid(kind, &path) {
            return Ok(Navigation::Value(BackupSource { kind, path }));
        }
        let expected = match kind {
            BackupSourceKind::File => "file",
            BackupSourceKind::Folder => "folder",
        };
        let error = simple_error(format!(
            "the selected {expected} path is invalid: {}",
            path.display()
        ));
        print_error(&*error);
    }
}

fn backup_source_is_valid(kind: BackupSourceKind, path: &Path) -> bool {
    match kind {
        BackupSourceKind::File => path.is_file(),
        BackupSourceKind::Folder => path.is_dir(),
    }
}

fn print_backup_confirmation(cli: &Cli, source_label: &str) {
    v_concat_println!(
        "\n{}",
        v_concat!(
            "Backup summary\n  Source: {source_label}\n  Archive: {}\n  Compression level: {}\n  Hashing jobs: {}\n  Recursive: {}\n  Overwrite: {}",
            cli.to.display(),
            cli.compression_level,
            cli.jobs,
            if cli.no_recursive { "no" } else { "yes" },
            if cli.overwrite { "yes" } else { "no" }
        )
    );
}

fn restore_backup_flow(theme: &InteractiveTheme) -> Result<Navigation<()>> {
    let mut archive: Option<PathBuf> = None;
    let mut destination: Option<PathBuf> = None;
    let mut overwrite: Option<bool> = None;
    let mut step = 0_u8;

    loop {
        match step {
            0 => match read_path_navigation("Type or paste the archive full path", None)? {
                Navigation::Value(path) if path.is_file() => {
                    archive = Some(path);
                    step = 1;
                }
                Navigation::Value(path) => {
                    let error = simple_error(format!(
                        "the selected archive path is not a file: {}",
                        path.display()
                    ));
                    print_error(&*error);
                }
                Navigation::Back => return Ok(Navigation::Back),
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            1 => {
                let default = default_restore_path(
                    archive
                        .as_ref()
                        .expect("archive selected before destination"),
                );
                match read_path_navigation(
                    "Type or paste the restore folder full path",
                    Some(&default),
                )? {
                    Navigation::Value(path) => {
                        destination = Some(path);
                        step = 2;
                    }
                    Navigation::Back => step = 0,
                    Navigation::Quit => return Ok(Navigation::Quit),
                }
            }
            2 => match yes_no_navigation(theme, "Overwrite existing files?", false)? {
                Navigation::Value(value) => {
                    overwrite = Some(value);
                    step = 3;
                }
                Navigation::Back => step = 1,
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            _ => {
                let archive = archive
                    .as_ref()
                    .expect("archive selected before confirmation");
                let destination = destination
                    .as_ref()
                    .expect("destination selected before confirmation");
                let overwrite = overwrite.expect("overwrite selected before confirmation");
                print_padded_stdout(v_concat!(
                    "Restore summary\n  Archive: {}\n  Destination: {}\n  Overwrite: {}",
                    archive.display(),
                    destination.display(),
                    if overwrite { "yes" } else { "no" }
                ));
                match select_navigation(
                    theme,
                    "Ready to restore the backup",
                    &["Start restore", "Back", "Quit"],
                    0,
                )? {
                    Navigation::Value(0) => {
                        let mut cli = Cli::default();
                        cli.restore = Some(archive.clone());
                        cli.to = destination.clone();
                        cli.overwrite = overwrite;
                        run(cli)?;
                        return Ok(Navigation::Value(()));
                    }
                    Navigation::Value(1) | Navigation::Back => step = 2,
                    Navigation::Value(_) | Navigation::Quit => return Ok(Navigation::Quit),
                }
            }
        }
    }
}

fn configure_backup_options(theme: &InteractiveTheme, cli: &mut Cli) -> Result<Navigation<()>> {
    let mut step = 0_u8;
    loop {
        match step {
            0 => match choose_compression_level(theme)? {
                Navigation::Value(value) => {
                    cli.compression_level = value;
                    step = 1;
                }
                Navigation::Back => return Ok(Navigation::Back),
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            1 => match choose_hashing_jobs(theme)? {
                Navigation::Value(value) => {
                    cli.jobs = value;
                    step = 2;
                }
                Navigation::Back => step = 0,
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            2 => match yes_no_navigation(theme, "Include subdirectories recursively?", true)? {
                Navigation::Value(value) => {
                    cli.no_recursive = !value;
                    step = 3;
                }
                Navigation::Back => step = 1,
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            _ => match yes_no_navigation(
                theme,
                "Overwrite the backup file if it already exists?",
                false,
            )? {
                Navigation::Value(value) => {
                    cli.overwrite = value;
                    return Ok(Navigation::Value(()));
                }
                Navigation::Back => step = 2,
                Navigation::Quit => return Ok(Navigation::Quit),
            },
        }
    }
}

fn choose_compression_level(theme: &InteractiveTheme) -> Result<Navigation<i32>> {
    loop {
        match select_navigation(
            theme,
            "Compression",
            &[
                "Balanced (level 6)",
                "Fast (level 1)",
                "High (level 12)",
                "Maximum (level 22)",
                "Custom level",
                "Back",
                "Quit",
            ],
            0,
        )? {
            Navigation::Value(0) => return Ok(Navigation::Value(6)),
            Navigation::Value(1) => return Ok(Navigation::Value(1)),
            Navigation::Value(2) => return Ok(Navigation::Value(12)),
            Navigation::Value(3) => return Ok(Navigation::Value(22)),
            Navigation::Value(4) => match read_text_navigation("Compression level (0-22)")? {
                Navigation::Value(value) => match value.parse::<i32>() {
                    Ok(level)
                        if (MIN_COMPRESSION_LEVEL..=MAX_COMPRESSION_LEVEL).contains(&level) =>
                    {
                        return Ok(Navigation::Value(level));
                    }
                    _ => print_inline_error("compression level must be between 0 and 22"),
                },
                Navigation::Back => continue,
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            Navigation::Value(5) | Navigation::Back => return Ok(Navigation::Back),
            Navigation::Value(_) | Navigation::Quit => return Ok(Navigation::Quit),
        }
    }
}

fn choose_hashing_jobs(theme: &InteractiveTheme) -> Result<Navigation<usize>> {
    let presets = hashing_job_presets();
    let recommended = v_concat!(
        "Recommended ({}) - automatic for this CPU",
        presets.recommended
    );
    let max = v_concat!("Max ({}) - max resources, potentially faster", presets.max);
    let medium = v_concat!("Medium ({}) - normal resources and speed", presets.medium);
    let low = v_concat!("Low ({}) - low resources, slower backups", presets.low);
    let custom = v_concat!("Custom (1-{}) - any value up to your CPU limit", presets.max);
    let custom_prompt = v_concat!("Number of hashing jobs (1-{})", presets.max);
    loop {
        match select_navigation(
            theme,
            "Parallel hashing jobs (duplicates detection)",
            &[
                recommended.as_str(),
                max.as_str(),
                medium.as_str(),
                low.as_str(),
                custom.as_str(),
                "Back",
                "Quit",
            ],
            0,
        )? {
            Navigation::Value(0) => return Ok(Navigation::Value(presets.recommended)),
            Navigation::Value(1) => return Ok(Navigation::Value(presets.max)),
            Navigation::Value(2) => return Ok(Navigation::Value(presets.medium)),
            Navigation::Value(3) => return Ok(Navigation::Value(presets.low)),
            Navigation::Value(4) => match read_text_navigation(&custom_prompt)? {
                Navigation::Value(value) => match value.parse::<usize>() {
                    Ok(jobs) if (1..=presets.max).contains(&jobs) => {
                        return Ok(Navigation::Value(jobs));
                    }
                    _ => print_inline_error(&v_concat!(
                        "parallel hashing jobs must be between 1 and {}",
                        presets.max
                    )),
                },
                Navigation::Back => continue,
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            Navigation::Value(5) | Navigation::Back => return Ok(Navigation::Back),
            Navigation::Value(_) | Navigation::Quit => return Ok(Navigation::Quit),
        }
    }
}

fn configure_exclusions(theme: &InteractiveTheme, cli: &mut Cli) -> Result<Navigation<()>> {
    let mut choosing = false;
    loop {
        if !choosing {
            match yes_no_navigation(theme, "Add exclusions?", false)? {
                Navigation::Value(false) => return Ok(Navigation::Value(())),
                Navigation::Value(true) => choosing = true,
                Navigation::Back => return Ok(Navigation::Back),
                Navigation::Quit => return Ok(Navigation::Quit),
            }
            continue;
        }

        let kind = select_navigation(
            theme,
            "Choose an exclusion",
            &[
                "Exclude a file name or path",
                "Exclude a folder name or path",
                "Exclude paths matching a regular expression",
                "Done adding exclusions",
                "Back",
                "Quit",
            ],
            3,
        )?;
        match kind {
            Navigation::Value(0) => match read_path_navigation("File name or path", None)? {
                Navigation::Value(path) => cli.exclude_file.push(path.display().to_string()),
                Navigation::Back => {}
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            Navigation::Value(1) => match read_path_navigation("Folder name or path", None)? {
                Navigation::Value(path) => cli.exclude_dir.push(path.display().to_string()),
                Navigation::Back => {}
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            Navigation::Value(2) => match read_text_navigation("Exclusion regular expression")? {
                Navigation::Value(value) => cli.exclude_regex.push(value),
                Navigation::Back => {}
                Navigation::Quit => return Ok(Navigation::Quit),
            },
            Navigation::Value(3) => return Ok(Navigation::Value(())),
            Navigation::Value(4) | Navigation::Back => choosing = false,
            Navigation::Value(_) | Navigation::Quit => return Ok(Navigation::Quit),
        }
    }
}

fn print_inline_error(message: &str) {
    let error = simple_error(message);
    print_error(&*error);
}

fn clean_interactive_path(input: &str) -> String {
    let trimmed = input.trim();
    let unquoted = if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    let mut cleaned = String::with_capacity(unquoted.len());
    let mut chars = unquoted.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.peek().copied() {
                Some(next) if next.is_whitespace() => {
                    cleaned.push(next);
                    chars.next();
                }
                _ => cleaned.push(character),
            }
        } else {
            cleaned.push(character);
        }
    }
    cleaned
}

fn default_archive_path(source: &Path) -> PathBuf {
    if source.file_name().is_none() {
        return PathBuf::from("backup.fsb");
    }
    if source
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(ARCHIVE_EXTENSION))
    {
        let stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("backup");
        return source.with_file_name(format!("{stem}_backup.fsb"));
    }
    source.with_extension(ARCHIVE_EXTENSION)
}

fn default_restore_path(archive: &Path) -> PathBuf {
    let stem = archive
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("restored");
    archive.with_file_name(format!("{stem}_restored"))
}

fn check_update_flow() -> Result<()> {
    print_padded_stdout(updater::check_update()?);
    Ok(())
}

fn update_flow() -> Result<()> {
    print_padded_stdout(updater::install_update()?);
    Ok(())
}

fn wait_for_interactive_enter() {
    v_concat_println!("\n  Press Enter to continue...\n");
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    v_concat_println!();
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
    v_concat_println!("\n{}", v_concat!(
        "{ANSI_CYAN}{BANNER_COMPRESSION_INPUT}{ANSI_YELLOW}{BANNER_COMPRESSION_OUTPUT}{ANSI_BLUE}{BANNER_WORDMARK}{ANSI_RESET}\
 {ANSI_CYAN}v_fs_backup{ANSI_RESET} - Fast, compressed, metadata-preserving filesystem backups."
    ));
}
