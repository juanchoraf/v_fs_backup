fn color(code: &str, text: impl AsRef<str>) -> String {
    format!("{code}{}{ANSI_RESET}", text.as_ref())
}

fn fact_line(label: &str, value: impl AsRef<str>) -> String {
    fact_line_with_value_color(label, value, ANSI_GREEN)
}

fn fact_line_with_value_color(
    label: &str,
    value: impl AsRef<str>,
    value_color: &str,
) -> String {
    let label = format!("{label:<26}:");
    format!("  {} {}", color(ANSI_CYAN, label), color(value_color, value))
}

fn print_backup_summary(stats: &BackupStats, elapsed: Duration) {
    let mut output = color(ANSI_GREEN, "Backup complete");
    output.push('\n');
    output.push_str(&fact_line("Entries", stats.entries.to_string()));
    output.push('\n');
    output.push_str(&fact_line("Input", human_bytes(stats.original_bytes)));
    output.push('\n');
    output.push_str(&fact_line(
        "Stored before compression",
        human_bytes(stats.stored_file_bytes),
    ));
    output.push('\n');
    output.push_str(&fact_line_with_value_color(
        "Backup file size",
        human_bytes(stats.archive_bytes),
        ANSI_YELLOW,
    ));
    if stats.deduplicated_bytes > 0 {
        output.push('\n');
        output.push_str(&fact_line(
            "Deduplicated",
            format!(
                "{} of duplicate file content",
                human_bytes(stats.deduplicated_bytes)
            ),
        ));
    }
    output.push('\n');
    output.push_str(&fact_line_with_value_color(
        "Backup file",
        stats.archive_path.display().to_string(),
        ANSI_YELLOW,
    ));
    output.push('\n');
    output.push_str(&fact_line("Total time", human_duration(elapsed)));
    print_padded_stderr(output);
}

fn print_restore_summary(stats: &RestoreStats, elapsed: Duration) {
    let mut output = color(ANSI_GREEN, "Restore complete");
    output.push('\n');
    output.push_str(&fact_line("Entries", stats.entries.to_string()));
    output.push('\n');
    output.push_str(&fact_line("Restored", human_bytes(stats.restored_bytes)));
    output.push('\n');
    output.push_str(&fact_line("Total time", human_duration(elapsed)));
    print_padded_stderr(output);
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.2} {}", UNITS[unit])
    }
}

fn print_sized_progress(label: &str, bytes: u64, path: &str, note: Option<&str>) {
    let size = format!("{:>12}", human_bytes(bytes));
    let suffix = note
        .map(|value| format!(" {}", color(ANSI_YELLOW, format!("({value})"))))
        .unwrap_or_default();
    v_concat_eprintln!(
        "{} ({}) {}{}",
        label,
        color(ANSI_GREEN, size),
        path,
        suffix
    );
}

fn print_time_row(label: &str, duration: Duration) {
    print_padded_stderr(color(
        ANSI_YELLOW,
        format!("{:<14} {}", label, human_duration(duration)),
    ));
}

fn print_time_row_without_bottom_space(label: &str, duration: Duration) {
    print_top_padded_stderr(color(
        ANSI_YELLOW,
        format!("{:<14} {}", label, human_duration(duration)),
    ));
}

fn human_duration(duration: Duration) -> String {
    if duration.as_secs() == 0 {
        return format!("{}ms", duration.as_millis());
    }

    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}
