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
            Ok(args) => match parse_command_from(args) {
                Ok(Some(command)) => {
                    if let Err(error) = run_parsed_command(command) {
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
v_fs_backup: fast compressed filesystem backups. Type help, compress, decompress, install, update, clear, or exit."
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
        "check-update" => args.push(OsString::from("--check-update")),
        "update" => args.push(OsString::from("--update")),
        "install" => args.push(OsString::from("--install")),
        "uninstall" => args.push(OsString::from("--uninstall")),
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
