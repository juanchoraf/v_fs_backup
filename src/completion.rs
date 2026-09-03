const INTERACTIVE_COMMANDS: &[&str] = &[
    "backup",
    "compress",
    "decompress",
    "restore",
    "check-update",
    "update",
    "install",
    "uninstall",
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
    "--check-update",
    "--update",
    "--install",
    "--uninstall",
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
