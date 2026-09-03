impl ArchiveProgressMonitor {
    fn start(
        enabled: bool,
        label: &'static str,
        subject: Option<String>,
        compressed_bytes: Arc<AtomicU64>,
    ) -> Self {
        let started = Instant::now();
        let (stop, stop_rx) = mpsc::channel();
        let handle = enabled.then(|| {
            let compressed_bytes = Arc::clone(&compressed_bytes);
            let subject = subject.clone();
            thread::spawn(move || {
                while stop_rx
                    .recv_timeout(ARCHIVE_SAVE_PROGRESS_INTERVAL)
                    .is_err()
                {
                    print_archive_progress(
                        label,
                        subject.as_deref(),
                        compressed_bytes.load(Ordering::Relaxed),
                        started.elapsed(),
                    );
                }
            })
        });

        Self {
            enabled,
            label,
            subject,
            stop: enabled.then_some(stop),
            handle,
            started,
            compressed_bytes,
        }
    }

    fn finish(mut self) {
        if !self.enabled {
            return;
        }
        self.stop_thread();
        print_archive_progress(
            self.label,
            self.subject.as_deref(),
            self.compressed_bytes.load(Ordering::Relaxed),
            self.started.elapsed(),
        );
    }

    fn stop_thread(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ArchiveProgressMonitor {
    fn drop(&mut self) {
        if self.enabled {
            self.stop_thread();
        }
    }
}

fn print_archive_progress(
    label: &str,
    subject: Option<&str>,
    compressed_bytes: u64,
    elapsed: Duration,
) {
    let mut output = color(ANSI_CYAN, label);
    if let Some(subject) = subject {
        output.push('\n');
        output.push_str(&fact_line("Archive", subject));
        output.push('\n');
        output.push_str(&fact_line("Read", human_bytes(compressed_bytes)));
    } else {
        output.push('\n');
        output.push_str(&fact_line(
            "Compressed archive",
            human_bytes(compressed_bytes),
        ));
    }
    output.push('\n');
    output.push_str(&fact_line("Elapsed", human_duration(elapsed)));
    print_padded_stderr(output);
}

fn progress_file_name(path: &Path) -> String {
    path.file_name()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
