pub fn run_from_env() -> Result<()> {
    initialize_terminal();
    let args: Vec<OsString> = env::args_os().collect();
    if args.len() == 1 {
        if installer::should_auto_install_current_executable()? {
            print_padded_stdout(installer::install_current_executable()?);
            return Ok(());
        }
        if relaunch_interactive_in_powershell()? {
            return Ok(());
        }
        return run_interactive_shell();
    }

    let Some(command) = parse_command_from(args)? else {
        return Ok(());
    };
    run_parsed_command(command)
}

fn run_parsed_command(command: ParsedCommand) -> Result<()> {
    match command {
        ParsedCommand::Run(cli) => run(cli),
        ParsedCommand::CheckUpdate => {
            print_padded_stdout(updater::check_update()?);
            Ok(())
        }
        ParsedCommand::Update => {
            print_padded_stdout(updater::install_update()?);
            Ok(())
        }
        ParsedCommand::Install => {
            print_padded_stdout(installer::install_current_executable()?);
            Ok(())
        }
        ParsedCommand::Uninstall => {
            print_padded_stdout(installer::uninstall_current_installation()?);
            Ok(())
        }
    }
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
