#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HashingJobPresets {
    recommended: usize,
    max: usize,
    medium: usize,
    low: usize,
}

fn default_jobs() -> usize {
    max_hashing_jobs()
}

fn max_hashing_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
}

fn hashing_job_presets() -> HashingJobPresets {
    hashing_job_presets_for(max_hashing_jobs())
}

fn hashing_job_presets_for(max: usize) -> HashingJobPresets {
    let max = max.max(1);
    HashingJobPresets {
        recommended: max,
        max,
        medium: percentage_of_jobs(max, 45),
        low: percentage_of_jobs(max, 10),
    }
}

fn percentage_of_jobs(max: usize, percentage: usize) -> usize {
    ((max * percentage + 50) / 100).clamp(1, max)
}
