#[derive(Debug, Clone, PartialEq, Eq)]
enum Navigation<T> {
    Value(T),
    Back,
    Quit,
}

struct InteractiveTheme {
    base: ColorfulTheme,
}

impl Theme for InteractiveTheme {
    fn format_select_prompt(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
    ) -> std::fmt::Result {
        self.base.format_select_prompt(f, prompt)
    }

    fn format_select_prompt_selection(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        selection: &str,
    ) -> std::fmt::Result {
        if let Some(code) = navigation_item_color(selection, true) {
            return self
                .base
                .format_select_prompt_selection(f, prompt, &color(code, selection));
        }
        self.base
            .format_select_prompt_selection(f, prompt, selection)
    }

    fn format_select_prompt_item(
        &self,
        f: &mut dyn std::fmt::Write,
        text: &str,
        active: bool,
    ) -> std::fmt::Result {
        let Some(code) = navigation_item_color(text, active) else {
            return self.base.format_select_prompt_item(f, text, active);
        };
        let prefix = if active {
            &self.base.active_item_prefix
        } else {
            &self.base.inactive_item_prefix
        };
        write!(f, "{} {}", prefix, color(code, text))
    }
}

fn navigation_item_color(item: &str, active: bool) -> Option<&'static str> {
    match (item, active) {
        ("Back" | "Clear screen", true) => Some(ANSI_BRIGHT_YELLOW),
        ("Back" | "Clear screen", false) => Some(ANSI_YELLOW),
        ("Quit", true) => Some(ANSI_BRIGHT_RED),
        ("Quit", false) => Some(ANSI_RED),
        _ => None,
    }
}

fn interactive_theme() -> InteractiveTheme {
    let mut base = ColorfulTheme::default();
    base.prompt_prefix = dialoguer::console::style("◆".to_owned())
        .for_stderr()
        .cyan();
    base.prompt_suffix = dialoguer::console::style("\n".to_owned()).for_stderr();
    InteractiveTheme { base }
}

fn select_navigation(
    theme: &InteractiveTheme,
    prompt: &str,
    items: &[&str],
    default: usize,
) -> Result<Navigation<usize>> {
    print_question_spacer();
    let selection = Select::with_theme(theme)
        .with_prompt(prompt)
        .items(items)
        .default(default)
        .interact_opt()?;
    if selection.is_some_and(|index| items.get(index) == Some(&"Quit")) {
        v_concat_eprintln!();
        let _ = io::stderr().flush();
    }
    Ok(selection.map_or(Navigation::Back, Navigation::Value))
}

fn yes_no_navigation(
    theme: &InteractiveTheme,
    prompt: &str,
    default_yes: bool,
) -> Result<Navigation<bool>> {
    let default = usize::from(!default_yes);
    match select_navigation(theme, prompt, &["Yes", "No", "Back", "Quit"], default)? {
        Navigation::Value(0) => Ok(Navigation::Value(true)),
        Navigation::Value(1) => Ok(Navigation::Value(false)),
        Navigation::Value(2) | Navigation::Back => Ok(Navigation::Back),
        Navigation::Value(_) | Navigation::Quit => Ok(Navigation::Quit),
    }
}

fn read_path_navigation(label: &str, default: Option<&Path>) -> Result<Navigation<PathBuf>> {
    let config = interactive_editor_config();
    let mut editor =
        rustyline::Editor::<PathPromptHelper, rustyline::history::DefaultHistory>::with_config(
            config,
        )
        .map_err(|error| simple_error(format!("failed to start path prompt: {error}")))?;
    editor.set_helper(Some(PathPromptHelper));
    bind_escape_to_back(&mut editor);

    let prompt = input_question_prompt(label);
    let line = match default {
        Some(path) => {
            let initial = path.display().to_string();
            editor.readline_with_initial(&prompt, (&initial, ""))
        }
        None => editor.readline(&prompt),
    };
    let value = match line {
        Ok(value) => value,
        Err(rustyline::error::ReadlineError::Interrupted) => return Ok(Navigation::Back),
        Err(rustyline::error::ReadlineError::Eof) => return Ok(Navigation::Quit),
        Err(error) => bail!("failed to read path: {error}"),
    };
    let value = clean_interactive_path(&value);
    if value.is_empty() {
        if let Some(path) = default {
            return Ok(Navigation::Value(path.to_path_buf()));
        }
        bail!("{label} cannot be empty");
    }
    Ok(Navigation::Value(PathBuf::from(value)))
}

fn read_text_navigation(label: &str) -> Result<Navigation<String>> {
    let config = interactive_editor_config();
    let mut editor =
        rustyline::Editor::<(), rustyline::history::DefaultHistory>::with_config(config)
            .map_err(|error| simple_error(format!("failed to start text prompt: {error}")))?;
    bind_escape_to_back(&mut editor);

    let prompt = input_question_prompt(label);
    let value = match editor.readline(&prompt) {
        Ok(value) => value.trim().to_owned(),
        Err(rustyline::error::ReadlineError::Interrupted) => return Ok(Navigation::Back),
        Err(rustyline::error::ReadlineError::Eof) => return Ok(Navigation::Quit),
        Err(error) => bail!("failed to read input: {error}"),
    };
    if value.is_empty() {
        bail!("{label} cannot be empty");
    }
    Ok(Navigation::Value(value))
}

fn interactive_editor_config() -> rustyline::Config {
    rustyline::Config::builder()
        .color_mode(rustyline::ColorMode::Forced)
        .completion_type(rustyline::CompletionType::List)
        .keyseq_timeout(Some(100))
        .build()
}

fn bind_escape_to_back<H: rustyline::Helper>(
    editor: &mut rustyline::Editor<H, rustyline::history::DefaultHistory>,
) {
    let escape = rustyline::KeyEvent(rustyline::KeyCode::Esc, rustyline::Modifiers::NONE);
    let _ = editor.bind_sequence(escape, rustyline::Cmd::Interrupt);
}

fn print_question_spacer() {
    v_concat_eprintln!();
    let _ = io::stderr().flush();
}

fn input_question_prompt(label: &str) -> String {
    print_question_spacer();
    if label.ends_with('?') {
        v_concat!("  ◆ {} ", label)
    } else {
        v_concat!("  ◆ {}: ", label)
    }
}
