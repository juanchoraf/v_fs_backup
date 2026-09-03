pub fn restore_archive(request: RestoreRequest) -> Result<RestoreStats> {
    if request.archive.as_os_str().is_empty() {
        bail!("--restore requires an archive path");
    }
    fs::create_dir_all(&request.to).with_context(|| {
        format!(
            "failed to create restore directory {}",
            request.to.display()
        )
    })?;

    let archive = File::open(&request.archive)
        .with_context(|| format!("failed to open archive {}", request.archive.display()))?;
    let mut reader = BufReader::new(archive);
    let mut magic = [0_u8; MAGIC.len()];
    reader
        .read_exact(&mut magic)
        .with_context(|| format!("{} is not a v_fs_backup archive", request.archive.display()))?;
    let compression = match &magic {
        MAGIC => ArchiveCompression::Zstd,
        LEGACY_RLE_MAGIC => ArchiveCompression::LegacyRle,
        _ => bail!("{} is not a v_fs_backup archive", request.archive.display()),
    };

    let compressed_bytes = Arc::new(AtomicU64::new(magic.len() as u64));
    let counting_reader = CountingReader::new(reader, Arc::clone(&compressed_bytes));
    let mut decoder: Box<dyn Read> = match compression {
        ArchiveCompression::Zstd => Box::new(
            zstd::stream::read::Decoder::new(counting_reader)
                .context("failed to create zstd decoder")?,
        ),
        ArchiveCompression::LegacyRle => Box::new(RleDecoder::new(counting_reader)),
    };
    let inflate_monitor = ArchiveProgressMonitor::start(
        !request.quiet,
        "Inflating",
        Some(progress_file_name(&request.archive)),
        Arc::clone(&compressed_bytes),
    );
    let mut restored_by_hash: HashMap<String, PathBuf> = HashMap::new();
    let mut directories_for_metadata = Vec::new();
    let mut stats = RestoreStats::default();
    let mut archive_source_os = None;

    while let Some((tag, json)) = read_json_record(&mut decoder)? {
        match tag {
            TAG_MANIFEST => {
                let manifest = manifest_from_slice(&json)?;
                let supported_version = manifest.format_version == FORMAT_VERSION
                    || (compression == ArchiveCompression::LegacyRle
                        && manifest.format_version == LEGACY_FORMAT_VERSION);
                if !supported_version {
                    bail!(
                        "unsupported archive format version {}; this binary supports {}",
                        manifest.format_version,
                        FORMAT_VERSION
                    );
                }
                archive_source_os = Some(manifest.source_os);
            }
            TAG_DIRECTORY => {
                let meta = entry_metadata_from_slice_for_os(
                    &json,
                    require_archive_source_os(&archive_source_os)?,
                )?;
                let target = safe_restore_path(&request.to, &meta.path)?;
                if !request.quiet {
                    print_padded_stderr(v_concat!("Restore Dir {}", target.display()));
                }
                fs::create_dir_all(&target)
                    .with_context(|| format!("failed to create directory {}", target.display()))?;
                directories_for_metadata.push((target, meta));
                stats.directories += 1;
                stats.entries += 1;
            }
            TAG_SYMLINK => {
                let record = symlink_record_from_slice_for_os(
                    &json,
                    require_archive_source_os(&archive_source_os)?,
                )?;
                let target = safe_restore_path(&request.to, &record.meta.path)?;
                create_parent_dir(&target)?;
                prepare_restore_target(&target, request.overwrite)?;
                if !request.quiet {
                    print_padded_stderr(v_concat!("Restore Link {}", target.display()));
                }
                if let Err(error) =
                    create_symlink(Path::new(&record.target), &target, record.target_is_dir)
                {
                    bail!("failed to restore symlink {}: {error}", target.display());
                }
                stats.symlinks += 1;
                stats.entries += 1;
            }
            TAG_FILE_DATA => {
                let record = file_data_record_from_slice_for_os(
                    &json,
                    require_archive_source_os(&archive_source_os)?,
                )?;
                let target = safe_restore_path(&request.to, &record.meta.path)?;
                create_parent_dir(&target)?;
                prepare_restore_target(&target, request.overwrite)?;
                if !request.quiet {
                    print_sized_progress(
                        "Restore File",
                        record.data_len,
                        &target.display().to_string(),
                        None,
                    );
                }
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .with_context(|| format!("failed to create file {}", target.display()))?;
                copy_exact_bytes(&mut decoder, &mut output, record.data_len)?;
                output
                    .flush()
                    .with_context(|| format!("failed to flush {}", target.display()))?;
                apply_metadata(&target, &record.meta)?;
                restored_by_hash.insert(record.hash, target);
                stats.files += 1;
                stats.entries += 1;
                stats.restored_bytes += record.data_len;
            }
            TAG_FILE_REF => {
                let record = file_ref_record_from_slice_for_os(
                    &json,
                    require_archive_source_os(&archive_source_os)?,
                )?;
                let target = safe_restore_path(&request.to, &record.meta.path)?;
                create_parent_dir(&target)?;
                prepare_restore_target(&target, request.overwrite)?;
                let original = restored_by_hash.get(&record.hash).with_context(|| {
                    format!(
                        "archive references duplicate file {} before original {}",
                        record.meta.path, record.original_path
                    )
                })?;
                if !request.quiet {
                    print_sized_progress(
                        "Restore File",
                        record.meta.len,
                        &target.display().to_string(),
                        Some("duplicate"),
                    );
                }
                let mut input = File::open(original).with_context(|| {
                    format!("failed to open duplicate file {}", original.display())
                })?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .with_context(|| format!("failed to create file {}", target.display()))?;
                copy_exact_bytes(&mut input, &mut output, record.meta.len)?;
                output
                    .flush()
                    .with_context(|| format!("failed to flush {}", target.display()))?;
                apply_metadata(&target, &record.meta)?;
                stats.files += 1;
                stats.entries += 1;
                stats.restored_bytes += record.meta.len;
            }
            other => bail!("unknown archive record tag {other}"),
        }
    }

    directories_for_metadata.sort_by_key(|entry| Reverse(entry.0.components().count()));
    for (path, meta) in directories_for_metadata {
        apply_metadata(&path, &meta)?;
    }

    inflate_monitor.finish();
    Ok(stats)
}
