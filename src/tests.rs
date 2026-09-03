use super::*;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn tempdir() -> io::Result<TestDir> {
    let mut path = env::temp_dir();
    path.push(format!(
        "v_fs_backup-test-{}-{}",
        std::process::id(),
        now_unix_seconds()
    ));
    let mut attempt = 0_u32;
    loop {
        let candidate = if attempt == 0 {
            path.clone()
        } else {
            PathBuf::from(format!("{}-{attempt}", path.display()))
        };
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(TestDir { path: candidate }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

fn foreign_source_os() -> &'static str {
    if cfg!(windows) { "linux" } else { "windows" }
}

fn write_foreign_platform_metadata_option(payload: &mut Vec<u8>) {
    if cfg!(windows) {
        write_u8(payload, 1);
        write_u32(payload, 0o100644);
        write_u32(payload, 1000);
        write_u32(payload, 1000);
        write_u64(payload, 0);
    } else {
        write_u8(payload, 1);
        write_u32(payload, 0x20);
    }
}

fn foreign_file_data_record_payload(path: &str, data_len: u64, hash: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    write_string(&mut payload, path);
    write_u8(&mut payload, entry_kind_to_byte(EntryKind::File));
    write_u64(&mut payload, data_len);
    write_bool(&mut payload, false);
    write_file_stamp_option(&mut payload, None);
    write_file_stamp_option(&mut payload, None);
    write_file_stamp_option(&mut payload, None);
    write_foreign_platform_metadata_option(&mut payload);
    write_string(&mut payload, hash);
    write_u64(&mut payload, data_len);
    payload
}

#[test]
fn backup_output_path_adds_fsb_when_missing() {
    assert_eq!(
        resolve_backup_output_path(Path::new("backup")).unwrap(),
        PathBuf::from("backup.fsb")
    );
    assert_eq!(
        resolve_backup_output_path(Path::new("backup.FSB")).unwrap(),
        PathBuf::from("backup.FSB")
    );
}

#[test]
fn backup_output_directory_uses_generated_fsb_file() {
    let tmp = tempdir().unwrap();
    let output = resolve_backup_output_path(tmp.path()).unwrap();

    assert_eq!(output.parent(), Some(tmp.path()));
    assert_eq!(
        output.extension().and_then(|value| value.to_str()),
        Some("fsb")
    );
}

#[test]
fn parses_requested_multi_letter_aliases() {
    let args = normalize_cli_args([
        OsString::from("v_fs_backup"),
        OsString::from("-nr"),
        OsString::from("-rx=/\\.png$/i"),
        OsString::from("-ed"),
        OsString::from(".git"),
        OsString::from("--to"),
        OsString::from("backup.fsb"),
    ]);
    let cli = Cli::parse_from(args);
    assert!(cli.no_recursive);
    assert_eq!(cli.regex, vec!["/\\.png$/i"]);
    assert_eq!(cli.exclude_dir, vec![".git"]);
}

#[test]
fn interactive_line_splits_quoted_paths() {
    let words = split_interactive_line(r#"compress "C:\Users\A B" "D:\Backups\one.fsb""#).unwrap();

    assert_eq!(
        words,
        vec![
            "compress".to_string(),
            r#"C:\Users\A B"#.to_string(),
            r#"D:\Backups\one.fsb"#.to_string(),
        ]
    );
}

#[test]
fn interactive_compress_shortcut_maps_to_cli_args() {
    let args = interactive_args_from_line(r#"compress "/data/src" "/backup/data.fsb""#).unwrap();
    let cli = Cli::parse_from(args);

    assert_eq!(cli.dir, vec!["/data/src"]);
    assert_eq!(cli.to, PathBuf::from("/backup/data.fsb"));
}

#[test]
fn interactive_decompress_shortcut_maps_to_restore_args() {
    let args = interactive_args_from_line(r#"decompress "backup.fsb" "restore dir""#).unwrap();
    let cli = Cli::parse_from(args);

    assert_eq!(cli.restore, Some(PathBuf::from("backup.fsb")));
    assert_eq!(cli.to, PathBuf::from("restore dir"));
}

#[test]
fn interactive_command_completion_includes_commands_and_flags() {
    let commands = command_completion_pairs("com");
    assert!(commands.iter().any(|pair| pair.replacement == "compress "));

    let flags = command_completion_pairs("--to");
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].replacement, "--to ");
}

#[test]
fn update_commands_parse_from_cli_and_interactive_input() {
    let cli_command = parse_command_from([
        OsString::from("v_fs_backup"),
        OsString::from("--check-update"),
    ])
    .unwrap()
    .unwrap();
    assert!(matches!(cli_command, ParsedCommand::CheckUpdate));

    let interactive_command = parse_command_from(interactive_args_from_line("update").unwrap())
        .unwrap()
        .unwrap();
    assert!(matches!(interactive_command, ParsedCommand::Update));
}

#[test]
fn update_commands_reject_backup_or_restore_options() {
    let error = parse_command_from([
        OsString::from("v_fs_backup"),
        OsString::from("--update"),
        OsString::from("--to"),
        OsString::from("backup.fsb"),
    ])
    .unwrap_err();

    assert!(error.to_string().contains("cannot be combined"));
}

#[test]
fn interactive_path_completion_quotes_paths_with_spaces() {
    let tmp = tempdir().unwrap();
    let spaced = tmp.path().join("Program Files");
    fs::create_dir(&spaced).unwrap();

    let prefix = tmp.path().join("Program").display().to_string();
    let token = CompletionToken {
        start: 0,
        unquoted: prefix,
        quote: None,
    };
    let pairs = path_completion_pairs(&token);
    let expected_display = format!("{}{}", spaced.display(), std::path::MAIN_SEPARATOR);
    let expected_replacement = format!("\"{expected_display}\"");

    assert!(pairs.iter().any(|pair| {
        pair.display == expected_display && pair.replacement == expected_replacement
    }));
    assert_eq!(
        quote_path_completion(r"C:\Program Files", None),
        r#""C:\Program Files""#
    );
}

#[test]
fn accepts_legacy_compression_spelling_and_level_22() {
    let args = normalize_cli_args([
        OsString::from("v_fs_backup"),
        OsString::from("--compresion-level=22"),
        OsString::from("--to"),
        OsString::from("backup.fsb"),
        OsString::from("source"),
    ]);
    let cli = Cli::parse_from(args);

    assert_eq!(cli.compression_level, 22);
    validate_cli(&cli).unwrap();
}

#[test]
fn slash_regex_flags_are_supported() {
    let rx = compile_user_regex("/photo\\.png/im").unwrap();
    assert!(rx.is_match("PHOTO.png"));
}

#[test]
fn local_sha256_matches_known_vector() {
    let mut hasher = Sha256State::new();
    hasher.update(b"abc");
    let digest = hasher.finalize();
    let mut hex = String::new();
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").unwrap();
    }
    assert_eq!(
        hex,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn backup_restore_roundtrip_includes_hidden_and_dedupes() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source");
    let hidden = source.join(".git");
    fs::create_dir_all(&hidden).unwrap();
    fs::write(source.join("a.txt"), b"same content").unwrap();
    fs::write(source.join("b.txt"), b"same content").unwrap();
    fs::write(hidden.join("config"), b"[core]\n").unwrap();

    let archive = tmp.path().join("backup.fsb");
    let stats = create_backup(BackupRequest {
        roots: vec![source.clone()],
        files: vec![],
        dirs: vec![],
        regexes: vec![],
        exclude_files: vec![],
        exclude_dirs: vec![],
        exclude_regexes: vec![],
        no_recursive: false,
        to: archive.clone(),
        compression_level: 3,
        jobs: 1,
        overwrite: false,
        quiet: true,
    })
    .unwrap();
    assert!(stats.deduplicated_bytes > 0);

    let restore_to = tmp.path().join("restore");
    restore_archive(RestoreRequest {
        archive,
        to: restore_to.clone(),
        overwrite: false,
        quiet: true,
    })
    .unwrap();

    assert_eq!(
        fs::read(restore_to.join("source").join(".git").join("config")).unwrap(),
        b"[core]\n"
    );
    assert_eq!(
        fs::read(restore_to.join("source").join("b.txt")).unwrap(),
        b"same content"
    );
}

#[test]
fn restores_archive_with_foreign_platform_metadata() {
    let tmp = tempdir().unwrap();
    let archive = tmp.path().join("foreign.fsb");
    let data = b"foreign metadata";
    let file = File::create(&archive).unwrap();
    let mut writer = BufWriter::new(file);
    writer.write_all(MAGIC).unwrap();
    let mut encoder = zstd::stream::write::Encoder::new(writer, 1).unwrap();

    write_json_record(
        &mut encoder,
        TAG_MANIFEST,
        manifest_to_value(&ArchiveManifest {
            format_version: FORMAT_VERSION,
            created_unix_seconds: 0,
            source_os: foreign_source_os().to_string(),
        }),
    )
    .unwrap();
    write_json_record(
        &mut encoder,
        TAG_FILE_DATA,
        foreign_file_data_record_payload("foreign.txt", data.len() as u64, "foreign-hash"),
    )
    .unwrap();
    encoder.write_all(data).unwrap();
    let mut writer = encoder.finish().unwrap();
    writer.flush().unwrap();

    let restore_to = tmp.path().join("restore");
    let stats = restore_archive(RestoreRequest {
        archive,
        to: restore_to.clone(),
        overwrite: false,
        quiet: true,
    })
    .unwrap();

    assert_eq!(stats.files, 1);
    assert_eq!(fs::read(restore_to.join("foreign.txt")).unwrap(), data);
}

#[test]
fn restores_legacy_rle_archive() {
    let tmp = tempdir().unwrap();
    let archive = tmp.path().join("legacy.fsb");
    let file = File::create(&archive).unwrap();
    let mut writer = BufWriter::new(file);
    writer.write_all(LEGACY_RLE_MAGIC).unwrap();
    let mut encoder = RleEncoder::new(writer, 6);
    let data = b"legacy rle";
    let meta = EntryMetadata {
        path: "legacy.txt".to_string(),
        kind: EntryKind::File,
        len: data.len() as u64,
        readonly: false,
        modified: None,
        accessed: None,
        created: None,
        #[cfg(unix)]
        unix: None,
        #[cfg(windows)]
        windows: None,
    };

    write_json_record(
        &mut encoder,
        TAG_MANIFEST,
        manifest_to_value(&ArchiveManifest {
            format_version: LEGACY_FORMAT_VERSION,
            created_unix_seconds: 0,
            source_os: "test".to_string(),
        }),
    )
    .unwrap();
    write_json_record(
        &mut encoder,
        TAG_FILE_DATA,
        file_data_record_to_value(&FileDataRecord {
            meta,
            hash: "legacy-hash".to_string(),
            data_len: data.len() as u64,
        }),
    )
    .unwrap();
    encoder.write_all(data).unwrap();
    let mut writer = encoder.finish().unwrap();
    writer.flush().unwrap();

    let restore_to = tmp.path().join("restore");
    let stats = restore_archive(RestoreRequest {
        archive,
        to: restore_to.clone(),
        overwrite: false,
        quiet: true,
    })
    .unwrap();

    assert_eq!(stats.files, 1);
    assert_eq!(fs::read(restore_to.join("legacy.txt")).unwrap(), data);
}

#[test]
fn excludes_directories_before_selection() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source");
    fs::create_dir_all(source.join("keep")).unwrap();
    fs::create_dir_all(source.join("skip")).unwrap();
    fs::write(source.join("keep").join("a.txt"), b"a").unwrap();
    fs::write(source.join("skip").join("b.txt"), b"b").unwrap();

    let archive = tmp.path().join("backup.fsb");
    create_backup(BackupRequest {
        roots: vec![source.clone()],
        files: vec![],
        dirs: vec![],
        regexes: vec![],
        exclude_files: vec![],
        exclude_dirs: vec!["skip".to_string()],
        exclude_regexes: vec![],
        no_recursive: false,
        to: archive.clone(),
        compression_level: 1,
        jobs: 1,
        overwrite: false,
        quiet: true,
    })
    .unwrap();

    let restore_to = tmp.path().join("restore");
    restore_archive(RestoreRequest {
        archive,
        to: restore_to.clone(),
        overwrite: false,
        quiet: true,
    })
    .unwrap();

    assert!(
        restore_to
            .join("source")
            .join("keep")
            .join("a.txt")
            .exists()
    );
    assert!(!restore_to.join("source").join("skip").exists());
}

#[test]
fn absolute_dir_selector_does_not_add_current_dir_search_root() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source");
    fs::create_dir_all(&source).unwrap();

    let request = BackupRequest {
        roots: vec![],
        files: vec![],
        dirs: vec![source.to_string_lossy().to_string()],
        regexes: vec![],
        exclude_files: vec![],
        exclude_dirs: vec![],
        exclude_regexes: vec![],
        no_recursive: false,
        to: tmp.path().join("backup.fsb"),
        compression_level: 1,
        jobs: 1,
        overwrite: false,
        quiet: true,
    };

    let selectors = Selectors::new(&request).unwrap();
    let roots = build_walk_roots(&request, &selectors).unwrap();

    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].start, source);
    assert!(roots[0].include_all);
}

#[test]
fn missing_direct_path_selector_fails_instead_of_searching_cwd() {
    let tmp = tempdir().unwrap();
    let missing = tmp.path().join("missing");
    let request = BackupRequest {
        roots: vec![],
        files: vec![],
        dirs: vec![missing.to_string_lossy().to_string()],
        regexes: vec![],
        exclude_files: vec![],
        exclude_dirs: vec![],
        exclude_regexes: vec![],
        no_recursive: false,
        to: tmp.path().join("backup.fsb"),
        compression_level: 1,
        jobs: 1,
        overwrite: false,
        quiet: true,
    };

    let selectors = Selectors::new(&request).unwrap();
    let error = build_walk_roots(&request, &selectors).unwrap_err();

    assert!(error.to_string().contains("does not exist"));
}
