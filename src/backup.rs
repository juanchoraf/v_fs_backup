pub fn create_backup(request: BackupRequest) -> Result<BackupStats> {
    let prepare_started = Instant::now();
    validate_backup_request(&request)?;
    let selectors = Selectors::new(&request)?;
    let walk_roots = build_walk_roots(&request, &selectors)?;
    if walk_roots.is_empty() {
        bail!("nothing to back up: no roots or selectors resolved to readable paths");
    }
    if !request.quiet {
        print_time_row("Prepare", prepare_started.elapsed());
    }

    if !request.quiet {
        print_padded_stderr(v_concat!(
            "Scanning {} root(s) with {} worker(s)...",
            walk_roots.len(),
            request.jobs
        ));
    }

    let scan_started = Instant::now();
    let mut entries =
        collect_entries(&walk_roots, &selectors, request.no_recursive, request.quiet)?;
    if !request.quiet {
        print_time_row("Scan", scan_started.elapsed());
    }
    if entries.is_empty() {
        bail!("nothing matched the backup request");
    }

    if !request.quiet {
        print_padded_stderr(v_concat!("Hashing {} selected entrie(s)...", entries.len()));
    }
    let hash_started = Instant::now();
    hash_files(&mut entries, request.jobs)?;
    if !request.quiet {
        print_time_row("Hash/Read", hash_started.elapsed());
    }

    let copy_compress_started = Instant::now();
    let archive_path = resolve_backup_output_path(&request.to)?;
    if let Some(parent) = archive_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create archive directory {}", parent.display()))?;
    }

    let output = OpenOptions::new()
        .write(true)
        .create(request.overwrite)
        .create_new(!request.overwrite)
        .truncate(request.overwrite)
        .open(&archive_path)
        .with_context(|| {
            if request.overwrite {
                format!("failed to open archive {}", archive_path.display())
            } else {
                format!(
                    "failed to create archive {}; pass --overwrite to replace it",
                    archive_path.display()
                )
            }
        })?;

    let mut writer = BufWriter::new(output);
    writer.write_all(MAGIC)?;
    let compressed_bytes = Arc::new(AtomicU64::new(MAGIC.len() as u64));
    let counting_writer = CountingWriter::new(writer, Arc::clone(&compressed_bytes));

    let mut encoder = zstd::stream::write::Encoder::new(counting_writer, request.compression_level)
        .context("failed to create zstd encoder")?;
    encoder
        .include_checksum(true)
        .context("failed to configure zstd checksum")?;

    write_json_record(
        &mut encoder,
        TAG_MANIFEST,
        manifest_to_value(&ArchiveManifest {
            format_version: FORMAT_VERSION,
            created_unix_seconds: now_unix_seconds(),
            source_os: env::consts::OS.to_string(),
        }),
    )?;

    let mut seen_hashes: HashMap<String, String> = HashMap::new();
    let mut stats = BackupStats {
        archive_path: archive_path.clone(),
        entries: entries.len(),
        ..BackupStats::default()
    };

    for entry in entries {
        match entry.kind {
            EntryKind::Directory => {
                stats.directories += 1;
                if !request.quiet {
                    print_padded_stderr(v_concat!("Copy Dir {}", entry.archive_path));
                }
                write_json_record(
                    &mut encoder,
                    TAG_DIRECTORY,
                    entry_metadata_to_value(&entry.metadata),
                )?;
            }
            EntryKind::Symlink => {
                stats.symlinks += 1;
                let target = fs::read_link(&entry.source_path).with_context(|| {
                    format!("failed to read symlink {}", entry.source_path.display())
                })?;
                if !request.quiet {
                    print_padded_stderr(v_concat!(
                        "Copy Link {} -> {}",
                        entry.archive_path,
                        target.display()
                    ));
                }
                let target_is_dir = fs::metadata(&entry.source_path).ok().map(|m| m.is_dir());
                write_json_record(
                    &mut encoder,
                    TAG_SYMLINK,
                    symlink_record_to_value(&SymlinkRecord {
                        meta: entry.metadata,
                        target: normalize_path_lossy(&target),
                        target_is_dir,
                    }),
                )?;
            }
            EntryKind::File => {
                stats.files += 1;
                stats.original_bytes += entry.metadata.len;
                let hash = entry
                    .hash
                    .clone()
                    .context("internal error: selected file was not hashed")?;
                if let Some(original_path) = seen_hashes.get(&hash) {
                    stats.deduplicated_bytes += entry.metadata.len;
                    if !request.quiet {
                        print_sized_progress(
                            "Copy File",
                            entry.metadata.len,
                            &entry.archive_path,
                            Some(&format!("duplicate of {original_path}")),
                        );
                    }
                    write_json_record(
                        &mut encoder,
                        TAG_FILE_REF,
                        file_ref_record_to_value(&FileRefRecord {
                            meta: entry.metadata,
                            hash,
                            original_path: original_path.clone(),
                        }),
                    )?;
                } else {
                    seen_hashes.insert(hash.clone(), entry.archive_path.clone());
                    stats.stored_file_bytes += entry.metadata.len;
                    if !request.quiet {
                        print_sized_progress(
                            "Copy File",
                            entry.metadata.len,
                            &entry.archive_path,
                            None,
                        );
                    }
                    write_json_record(
                        &mut encoder,
                        TAG_FILE_DATA,
                        file_data_record_to_value(&FileDataRecord {
                            meta: entry.metadata.clone(),
                            hash,
                            data_len: entry.metadata.len,
                        }),
                    )?;
                    let mut input = File::open(&entry.source_path).with_context(|| {
                        format!("failed to open file {}", entry.source_path.display())
                    })?;
                    copy_exact_bytes(&mut input, &mut encoder, entry.metadata.len)?;
                }
            }
        }
    }
    if !request.quiet {
        print_time_row("Copy/Compress", copy_compress_started.elapsed());
    }
    let save_archive_started = Instant::now();
    let save_monitor = ArchiveProgressMonitor::start(
        !request.quiet,
        "Deflating",
        None,
        Arc::clone(&compressed_bytes),
    );
    let mut writer = match encoder.finish().context("failed to finish zstd stream") {
        Ok(writer) => writer,
        Err(error) => {
            save_monitor.finish();
            return Err(error);
        }
    };
    let flush_result = writer.flush().context("failed to flush archive");
    save_monitor.finish();
    flush_result?;
    if !request.quiet {
        print_time_row("Save Archive", save_archive_started.elapsed());
    }
    stats.archive_bytes = fs::metadata(&archive_path)
        .with_context(|| format!("failed to stat archive {}", archive_path.display()))?
        .len();
    Ok(stats)
}
