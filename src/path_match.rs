fn archive_path_for(start: &Path, prefix: Option<&str>, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(start).ok()?;
    let rel = normalize_archive_path(rel);
    match (prefix, rel.is_empty()) {
        (Some(prefix), true) => Some(prefix.to_string()),
        (Some(prefix), false) => Some(format!("{prefix}/{rel}")),
        (None, true) => None,
        (None, false) => Some(rel),
    }
}

fn normalize_archive_path(path: &Path) -> String {
    normalize_path_lossy(path)
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string()
}

fn normalize_path_lossy(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_match_text(value: &str) -> String {
    let mut normalized = value.replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_string();
    }
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    #[cfg(windows)]
    {
        normalized = normalized.to_lowercase();
    }
    normalized
}

fn path_eq_or_under(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn path_eq_or_nested_match(path: &str, pattern: &str) -> bool {
    path == pattern
        || path_eq_or_under(path, pattern)
        || path
            .strip_suffix(pattern)
            .is_some_and(|prefix| prefix.ends_with('/'))
}

fn path_contains_component_path(path: &str, pattern: &str) -> bool {
    path == pattern
        || path.starts_with(&format!("{pattern}/"))
        || path.ends_with(&format!("/{pattern}"))
        || path.contains(&format!("/{pattern}/"))
}

fn parent_archive_paths(path: &str) -> Vec<String> {
    let mut parents = Vec::new();
    let mut current = path;
    while let Some((parent, _)) = current.rsplit_once('/') {
        if parent.is_empty() {
            break;
        }
        parents.push(parent.to_string());
        current = parent;
    }
    parents
}

fn is_under_excluded_directory(
    selectors: &Selectors,
    archive_path: &str,
    absolute_path: &Path,
) -> bool {
    let Some((parent_archive_path, _)) = archive_path.rsplit_once('/') else {
        return false;
    };
    let parent_absolute_path = absolute_path.parent().unwrap_or(absolute_path);
    let parent_name = parent_absolute_path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    selectors.excludes_dirish(parent_archive_path, parent_absolute_path, &parent_name)
}
