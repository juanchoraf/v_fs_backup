fn validate_cli(cli: &Cli) -> Result<()> {
    if cli.to.as_os_str().is_empty() {
        bail!("--to is required");
    }
    if !(MIN_COMPRESSION_LEVEL..=MAX_COMPRESSION_LEVEL).contains(&cli.compression_level) {
        bail!(
            "--compression-level must be between {MIN_COMPRESSION_LEVEL} and {MAX_COMPRESSION_LEVEL}"
        );
    }
    if cli.jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    if cli.restore.is_some()
        && (!cli.file.is_empty()
            || !cli.dir.is_empty()
            || !cli.regex.is_empty()
            || !cli.roots.is_empty())
    {
        bail!("--restore cannot be combined with backup selectors or search roots");
    }
    Ok(())
}

fn validate_backup_request(request: &BackupRequest) -> Result<()> {
    if request.to.as_os_str().is_empty() {
        bail!("backup destination is required");
    }
    if !(MIN_COMPRESSION_LEVEL..=MAX_COMPRESSION_LEVEL).contains(&request.compression_level) {
        bail!(
            "compression level must be between {MIN_COMPRESSION_LEVEL} and {MAX_COMPRESSION_LEVEL}"
        );
    }
    if request.jobs == 0 {
        bail!("jobs must be at least 1");
    }
    Ok(())
}

fn build_walk_roots(request: &BackupRequest, selectors: &Selectors) -> Result<Vec<WalkRoot>> {
    let roots_were_provided = !request.roots.is_empty();
    let mut needs_implicit_search_root = !request.regexes.is_empty();
    let mut walk_roots = Vec::new();

    for file in &request.files {
        if should_treat_as_direct_path(file, roots_were_provided) {
            let path = PathBuf::from(file);
            if !path.exists() {
                bail!("file selector path does not exist: {}", path.display());
            }
            let meta = fs::symlink_metadata(&path)
                .with_context(|| format!("failed to read metadata for {}", path.display()))?;
            if meta.file_type().is_file() || meta.file_type().is_symlink() {
                walk_roots.push(WalkRoot {
                    start: path,
                    archive_prefix: Some(file_name_or_root(file)?),
                    include_all: true,
                });
            } else {
                bail!(
                    "--file is not a regular file or symlink: {}",
                    path.display()
                );
            }
        } else {
            needs_implicit_search_root = true;
        }
    }

    for dir in &request.dirs {
        if should_treat_as_direct_path(dir, roots_were_provided) {
            let path = PathBuf::from(dir);
            if !path.exists() {
                bail!("directory selector path does not exist: {}", path.display());
            }
            let meta = fs::symlink_metadata(&path)
                .with_context(|| format!("failed to read metadata for {}", path.display()))?;
            if meta.file_type().is_dir() || meta.file_type().is_symlink() {
                walk_roots.push(WalkRoot {
                    start: path,
                    archive_prefix: Some(file_name_or_root(dir)?),
                    include_all: true,
                });
            } else {
                bail!("--dir is not a directory or symlink: {}", path.display());
            }
        } else {
            needs_implicit_search_root = true;
        }
    }

    let mut search_roots = request.roots.clone();
    if search_roots.is_empty()
        && (needs_implicit_search_root
            || (!selectors.has_include_filters() && walk_roots.is_empty()))
    {
        search_roots.push(env::current_dir().context("failed to resolve current directory")?);
    }

    let include_all_for_search_roots = !selectors.has_include_filters();
    for root in search_roots {
        if !root.exists() {
            bail!("search root does not exist: {}", root.display());
        }
        let prefix = if include_all_for_search_roots {
            Some(file_name_or_root_path(&root)?)
        } else {
            None
        };
        walk_roots.push(WalkRoot {
            start: root,
            archive_prefix: prefix,
            include_all: include_all_for_search_roots,
        });
    }

    Ok(deduplicate_walk_roots(walk_roots))
}

fn collect_entries(
    roots: &[WalkRoot],
    selectors: &Selectors,
    no_recursive: bool,
    quiet: bool,
) -> Result<Vec<ScannedEntry>> {
    let mut all_entries: HashMap<String, ScannedEntry> = HashMap::new();
    let mut selected_paths: HashSet<String> = HashSet::new();

    for root in roots {
        let mut selected_dir_prefixes: Vec<String> = Vec::new();
        let max_depth = if no_recursive { 1 } else { usize::MAX };
        let mut stack = vec![(root.start.clone(), 0_usize)];

        while let Some((path, depth)) = stack.pop() {
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    if quiet {
                        return Err(error)
                            .with_context(|| format!("failed to inspect {}", path.display()));
                    }
                    print_padded_stderr(v_concat!(
                        "skip unreadable entry: {}: {error}",
                        path.display()
                    ));
                    continue;
                }
            };

            let file_type = metadata.file_type();
            let kind = if file_type.is_dir() {
                EntryKind::Directory
            } else if file_type.is_file() {
                EntryKind::File
            } else if file_type.is_symlink() {
                EntryKind::Symlink
            } else {
                if !quiet {
                    print_padded_stderr(v_concat!("skip special file {}", path.display()));
                }
                continue;
            };

            let mut descend = kind == EntryKind::Directory && depth < max_depth;

            if let Some(archive_path) =
                archive_path_for(&root.start, root.archive_prefix.as_deref(), &path)
                    .filter(|path| !path.is_empty())
            {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default();
                let excluded_by_directory = if kind == EntryKind::Directory {
                    selectors.excludes_dirish(&archive_path, &path, &name)
                } else {
                    is_under_excluded_directory(selectors, &archive_path, &path)
                };

                let excluded = selectors.excludes_regex(&archive_path, &path)
                    || (kind == EntryKind::File
                        && selectors.excludes_file(&archive_path, &path, &name))
                    || excluded_by_directory;

                if excluded {
                    descend = false;
                } else {
                    let selected_by_ancestor = selected_dir_prefixes
                        .iter()
                        .any(|prefix| path_eq_or_under(&archive_path, prefix));
                    let selected = root.include_all
                        || selected_by_ancestor
                        || match kind {
                            EntryKind::File | EntryKind::Symlink => {
                                selectors.matches_include_file(&archive_path, &path, &name)
                                    || selectors.matches_include_regex(&archive_path, &path)
                            }
                            EntryKind::Directory => {
                                selectors.matches_include_dir(&archive_path, &path, &name)
                                    || selectors.matches_include_regex(&archive_path, &path)
                            }
                        };

                    if selected && kind == EntryKind::Directory {
                        selected_dir_prefixes.push(archive_path.clone());
                    }

                    let meta = metadata_for(&path, archive_path.clone(), kind, &metadata)?;
                    all_entries
                        .entry(archive_path.clone())
                        .or_insert(ScannedEntry {
                            source_path: path.clone(),
                            archive_path: archive_path.clone(),
                            kind,
                            metadata: meta,
                            hash: None,
                        });

                    if selected {
                        selected_paths.insert(archive_path);
                    }
                }
            }

            if descend {
                let mut children = match sorted_child_paths(&path) {
                    Ok(children) => children,
                    Err(error) => {
                        if quiet {
                            return Err(error)
                                .with_context(|| format!("failed to read {}", path.display()));
                        }
                        print_padded_stderr(v_concat!(
                            "skip unreadable directory: {}: {error}",
                            path.display()
                        ));
                        continue;
                    }
                };
                children.reverse();
                stack.extend(children.into_iter().map(|child| (child, depth + 1)));
            }
        }
    }

    let mut ancestor_paths = Vec::new();
    for selected in &selected_paths {
        ancestor_paths.extend(parent_archive_paths(selected));
    }
    for ancestor in ancestor_paths {
        if all_entries.contains_key(&ancestor) {
            selected_paths.insert(ancestor);
        }
    }

    let mut entries: Vec<ScannedEntry> = all_entries
        .into_iter()
        .filter_map(|(path, entry)| selected_paths.contains(&path).then_some(entry))
        .collect();
    entries.sort_by(|a, b| {
        a.archive_path
            .components_count()
            .cmp(&b.archive_path.components_count())
            .then_with(|| a.archive_path.cmp(&b.archive_path))
    });
    Ok(entries)
}
fn sorted_child_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut children = Vec::new();
    for entry in fs::read_dir(path)? {
        children.push(entry?.path());
    }
    children.sort_by(|left, right| {
        let left_name = left.file_name().unwrap_or_default();
        let right_name = right.file_name().unwrap_or_default();
        left_name.cmp(right_name)
    });
    Ok(children)
}

trait ArchivePathOrdering {
    fn components_count(&self) -> usize;
}

impl ArchivePathOrdering for str {
    fn components_count(&self) -> usize {
        self.split('/').filter(|part| !part.is_empty()).count()
    }
}
fn should_treat_as_direct_path(raw: &str, roots_were_provided: bool) -> bool {
    let path = Path::new(raw);
    path.is_absolute()
        || raw.contains('/')
        || raw.contains('\\')
        || (!roots_were_provided && path.exists())
}

fn file_name_or_root(raw: &str) -> Result<String> {
    file_name_or_root_path(Path::new(raw))
}

fn file_name_or_root_path(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .or_else(|| {
            path.components()
                .next_back()
                .map(|component| component.as_os_str())
        })
        .context("could not determine archive name for path")?;
    let value = normalize_archive_path(Path::new(name));
    if value.is_empty() {
        bail!("could not determine archive name for {}", path.display());
    }
    Ok(value)
}

fn deduplicate_walk_roots(roots: Vec<WalkRoot>) -> Vec<WalkRoot> {
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter(|root| {
            let key = format!(
                "{}\0{}\0{}",
                normalize_path_lossy(&root.start),
                root.archive_prefix.clone().unwrap_or_default(),
                root.include_all
            );
            seen.insert(key)
        })
        .collect()
}
