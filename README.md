# v_fs_backup

[![Release_Badge]][Release_Url]
[![Build_Badge]][Build_Url]
[![Dependencies_Badge]][Dependencies_Url]
![License_Badge](https://img.shields.io/badge/License-Apache--2.0_or_MIT-blue)

`v_fs_backup` is a fast, CLI-only backup tool for Linux, macOS, BSD/Unix-like
systems, and Windows. It creates compressed `.fsb` archives while preserving:

- Files, directories, and hidden entries such as `.git` and `.htaccess`
- Symlinks, timestamps, permissions, and supported platform metadata
- Duplicate file data efficiently through SHA-256-based deduplication

Archives use a zstd-compressed stream. Repeated files and build trees can
compress very well. Photos, videos, ZIP files, encrypted files, and other data
that is already compressed may not become much smaller.

## Interactive Console

Start an installed copy with:

```bash
v_fs_backup
```

The console supports command and path completion with `Tab`, including paths
that contain spaces.

```text
v_fs_backup> compress /path/to/source /backups/source.fsb
v_fs_backup> decompress /backups/source.fsb /path/to/restore
v_fs_backup> clear
v_fs_backup> exit
```

On Windows, the Start menu shortcut opens the console in PowerShell so ANSI
colors are visible.

## Command-Line Examples

The `--to` option is the archive path during backup and the destination
directory during restore. If a backup destination does not end in `.fsb`, the
extension is added automatically.

Back up a directory:

```bash
v_fs_backup --dir /path/to/source --to /backups/source.fsb
```

Back up every entry below a search root:

```bash
v_fs_backup /path/to/search --to /backups/everything.fsb
```

Back up one exact file:

```bash
v_fs_backup --file /path/to/photo.png --to /backups/photo.fsb
```

Find matching files or directories below a search root:

```bash
v_fs_backup --file photo.png /path/to/search --to /backups/photos.fsb
v_fs_backup --dir project /path/to/search --to /backups/projects.fsb
```

Select entries with a regular expression:

```bash
v_fs_backup --regex '/([^0-9]\.png)+/im' /path/to/search --to /backups/pngs.fsb
```

The `/pattern/flags` form accepts `i`, `m`, `s`, `x`, and `U`. The `g` flag is
accepted but ignored because Rust regex matching is not stateful.

Restore an archive:

```bash
v_fs_backup --restore /backups/source.fsb --to /path/to/restore
```

Add `--overwrite` when existing restored files may be replaced.

### Selection and Exclusion

Traversal is recursive by default and includes hidden entries.

```bash
# Search only the root's direct children
v_fs_backup --no-recursive /path/to/search --to /backups/top-level.fsb

# Exclude a directory subtree
v_fs_backup /path/to/search --to /backups/archive.fsb --exclude-dir node_modules

# Exclude a file
v_fs_backup /path/to/search --to /backups/archive.fsb --exclude-file secret.env

# Exclude paths matched by a regular expression
v_fs_backup /path/to/search --to /backups/archive.fsb --exclude-regex '/(^|/)target($|/)/'
```

Short aliases:

| Short | Long |
| --- | --- |
| `-nr` | `--no-recursive` |
| `-rx` | `--regex` |
| `-ef` | `--exclude-file` |
| `-ed` | `--exclude-dir` |
| `-er` | `--exclude-regex` |

The legacy misspelling `--compresion-level` is also accepted as an alias for
`--compression-level`.

### Performance

```bash
v_fs_backup /data --to /backups/data.fsb --jobs 8 --compression-level 8
```

- `--jobs` controls parallel file hashing. By default, one physical CPU is
  left free when possible.
- `--compression-level` accepts `0..22` and defaults to `6`. Level `0` selects
  zstd's default. Higher levels trade more CPU time and memory for size.
- `--quiet` suppresses per-entry progress output.

Backup and restore output includes phase timings, transferred sizes, archive
size, and total elapsed time. Stream finalization does not show a percentage
because zstd cannot provide a reliable remaining-work count for that phase.

## Restore Safety and Metadata

- Restore rejects absolute archive paths and `..` path traversal.
- Unix ownership is restored when permissions and the filesystem allow it.
  Otherwise, restore continues and applies the available timestamps and modes.
- Restoring Windows symlinks may require Developer Mode or administrator
  privileges.

---

## Updates

Use `--check-update` and `--update` to pull updates. The update checks the release source and installs the latest matching binary from `https://github.com/juanchoraf/v_fs_backup` over the current executable when a newer release is available. Releases provide direct OS binaries; no install or uninstall scripts are required.

## Credits

Powered by The Velasquez.

## License

The `v_fs_backup` library is distributed under either of

 * [Apache License, Version 2.0][LICENSE-APACHE]
 * [MIT license][LICENSE-MIT]

at your convenience.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

[//]: # (badges)

[Release_Badge]: https://github.com/juanchoraf/v_fs_backup/actions/workflows/release.yml/badge.svg
[Release_Url]: https://github.com/juanchoraf/v_fs_backup/actions/workflows/release.yml
[Build_Badge]: https://github.com/juanchoraf/v_fs_backup/actions/workflows/rust.yml/badge.svg?branch=main
[Build_Url]: https://github.com/juanchoraf/v_fs_backup/actions?query=branch:main
[Dependencies_Badge]: https://deps.rs/repo/github/juanchoraf/v_fs_backup/status.svg
[Dependencies_Url]: https://deps.rs/repo/github/juanchoraf/v_fs_backup

[//]: # (licenses)

[LICENSE-APACHE]: https://github.com/juanchoraf/v_fs_backup/blob/main/LICENSE-APACHE
[LICENSE-MIT]: https://github.com/juanchoraf/v_fs_backup/blob/main/LICENSE-MIT
