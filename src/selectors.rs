impl Regex {
    fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            case_insensitive: false,
        }
    }

    fn is_match(&self, value: &str) -> bool {
        let mut pattern = self.pattern.clone();
        let mut value = value.to_string();
        if self.case_insensitive {
            pattern = pattern.to_lowercase();
            value = value.to_lowercase();
        }

        if let Some(extension) = regex_extension_hint(&pattern) {
            return value.contains(&extension);
        }

        let anchored_start = pattern.starts_with('^');
        let anchored_end = pattern.ends_with('$') && !pattern.ends_with("\\$");
        if anchored_start {
            pattern.remove(0);
        }
        if anchored_end {
            pattern.pop();
        }

        let literal = regex_literal_hint(&pattern);
        if literal.contains('*') || literal.contains('?') {
            return wildcard_match(&literal, &value);
        }
        if anchored_start && anchored_end {
            value == literal
        } else if anchored_start {
            value.starts_with(&literal)
        } else if anchored_end {
            value.ends_with(&literal)
        } else {
            value.contains(&literal)
        }
    }
}

impl RegexBuilder {
    fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            case_insensitive: false,
        }
    }

    fn case_insensitive(&mut self, enabled: bool) -> &mut Self {
        self.case_insensitive = enabled;
        self
    }

    fn multi_line(&mut self, _enabled: bool) -> &mut Self {
        self
    }

    fn dot_matches_new_line(&mut self, _enabled: bool) -> &mut Self {
        self
    }

    fn ignore_whitespace(&mut self, _enabled: bool) -> &mut Self {
        self
    }

    fn swap_greed(&mut self, _enabled: bool) -> &mut Self {
        self
    }

    fn build(&self) -> Regex {
        Regex {
            pattern: self.pattern.clone(),
            case_insensitive: self.case_insensitive,
        }
    }
}

impl Selectors {
    fn new(request: &BackupRequest) -> Result<Self> {
        Ok(Self {
            include_files: request.files.iter().map(|s| PathSpec::new(s)).collect(),
            include_dirs: request.dirs.iter().map(|s| PathSpec::new(s)).collect(),
            include_regexes: compile_regexes(&request.regexes)?,
            exclude_files: request
                .exclude_files
                .iter()
                .map(|s| PathSpec::new(s))
                .collect(),
            exclude_dirs: request
                .exclude_dirs
                .iter()
                .map(|s| PathSpec::new(s))
                .collect(),
            exclude_regexes: compile_regexes(&request.exclude_regexes)?,
        })
    }

    fn has_include_filters(&self) -> bool {
        !(self.include_files.is_empty()
            && self.include_dirs.is_empty()
            && self.include_regexes.is_empty())
    }

    fn matches_include_file(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        self.include_files
            .iter()
            .any(|spec| spec.matches(archive_path, absolute_path, name))
    }

    fn matches_include_dir(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        self.include_dirs
            .iter()
            .any(|spec| spec.matches(archive_path, absolute_path, name))
    }

    fn matches_include_regex(&self, archive_path: &str, absolute_path: &Path) -> bool {
        let absolute = normalize_path_lossy(absolute_path);
        self.include_regexes
            .iter()
            .any(|rx| rx.is_match(archive_path) || rx.is_match(&absolute))
    }

    fn excludes_file(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        self.exclude_files
            .iter()
            .any(|spec| spec.matches(archive_path, absolute_path, name))
    }

    fn excludes_dirish(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        let absolute = normalize_path_lossy(absolute_path);
        self.exclude_dirs
            .iter()
            .any(|spec| spec.matches_dir_tree(archive_path, &absolute, name))
            || self
                .exclude_regexes
                .iter()
                .any(|rx| rx.is_match(archive_path) || rx.is_match(&absolute))
    }

    fn excludes_regex(&self, archive_path: &str, absolute_path: &Path) -> bool {
        let absolute = normalize_path_lossy(absolute_path);
        self.exclude_regexes
            .iter()
            .any(|rx| rx.is_match(archive_path) || rx.is_match(&absolute))
    }
}

impl PathSpec {
    fn new(raw: &str) -> Self {
        let path = Path::new(raw);
        let has_separator = raw.contains('/') || raw.contains('\\');
        Self {
            normalized: normalize_match_text(raw),
            has_separator,
            is_absolute: path.is_absolute(),
        }
    }

    fn matches(&self, archive_path: &str, absolute_path: &Path, name: &str) -> bool {
        if self.has_separator || self.is_absolute {
            let archive = normalize_match_text(archive_path);
            let absolute = normalize_match_text(&normalize_path_lossy(absolute_path));
            path_eq_or_nested_match(&archive, &self.normalized)
                || path_eq_or_nested_match(&absolute, &self.normalized)
        } else {
            normalize_match_text(name) == self.normalized
        }
    }

    fn matches_dir_tree(&self, archive_path: &str, absolute_path: &str, name: &str) -> bool {
        if self.matches(archive_path, Path::new(absolute_path), name) {
            return true;
        }
        if self.has_separator || self.is_absolute {
            let archive = normalize_match_text(archive_path);
            let absolute = normalize_match_text(absolute_path);
            path_contains_component_path(&archive, &self.normalized)
                || path_contains_component_path(&absolute, &self.normalized)
        } else {
            normalize_match_text(archive_path)
                .split('/')
                .any(|part| part == self.normalized)
        }
    }
}
fn compile_regexes(values: &[String]) -> Result<Vec<Regex>> {
    values
        .iter()
        .map(|value| compile_user_regex(value))
        .collect()
}

fn compile_user_regex(input: &str) -> Result<Regex> {
    if let Some((pattern, flags)) = parse_slash_regex(input) {
        let mut builder = RegexBuilder::new(pattern);
        for flag in flags.chars() {
            match flag {
                'g' => {}
                'i' => {
                    builder.case_insensitive(true);
                }
                'm' => {
                    builder.multi_line(true);
                }
                's' => {
                    builder.dot_matches_new_line(true);
                }
                'x' => {
                    builder.ignore_whitespace(true);
                }
                'U' => {
                    builder.swap_greed(true);
                }
                other => bail!("unsupported regex flag {other:?} in {input:?}"),
            }
        }
        Ok(builder.build())
    } else {
        Ok(Regex::new(input))
    }
}

fn regex_extension_hint(pattern: &str) -> Option<String> {
    let marker = pattern.rfind(r"\.")?;
    let mut extension = String::from(".");
    for ch in pattern[marker + 2..].chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            extension.push(ch);
        } else {
            break;
        }
    }
    (extension.len() > 1).then_some(extension)
}

fn regex_literal_hint(pattern: &str) -> String {
    let mut literal = String::new();
    let mut escaped = false;
    for ch in pattern.chars() {
        if escaped {
            literal.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '+' | '|') {
            continue;
        }
        literal.push(ch);
    }
    literal
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v) = (0, 0);
    let mut star = None;
    let mut star_value = 0;

    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            star_value = v;
        } else if let Some(star_idx) = star {
            p = star_idx + 1;
            star_value += 1;
            v = star_value;
        } else {
            return false;
        }
    }

    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn parse_slash_regex(input: &str) -> Option<(&str, &str)> {
    if !input.starts_with('/') {
        return None;
    }
    let mut escaped = false;
    let mut last_slash = None;
    for (idx, ch) in input.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '/' {
            last_slash = Some(idx);
        }
    }
    let idx = last_slash?;
    Some((&input[1..idx], &input[idx + 1..]))
}
