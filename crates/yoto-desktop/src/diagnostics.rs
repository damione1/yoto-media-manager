//! Keep panic details when Finder launches the app with stderr connected to /dev/null.
use std::{
    backtrace::Backtrace,
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

pub fn init(profile: &Path, version: &str) -> std::io::Result<()> {
    let directory = profile.join("logs");
    fs::create_dir_all(&directory)?;
    // Retain a small history across restarts, including the process that crashed.
    let mut previous = fs::read_dir(&directory)?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("desktop-") && name.ends_with(".log")
        })
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            Some((metadata.modified().ok()?, entry.path()))
        })
        .collect::<Vec<_>>();
    previous.sort_by_key(|(modified, _)| *modified);
    let expired = previous.len().saturating_sub(6);
    for (_, path) in previous.into_iter().take(expired) {
        let _ = fs::remove_file(path);
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(directory.join(format!("desktop-{}.log", std::process::id())))?;
    let _ = LOG.set(Mutex::new(file));
    record(format_args!(
        "Starting YotoMediaManager {version}, pid={}, executable={:?}, update_helper={}",
        std::process::id(),
        std::env::current_exe().ok(),
        std::env::args_os()
            .nth(1)
            .is_some_and(|arg| arg == "--apply-update")
    ));
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Capture inside the hook, before an Objective-C callback aborts unwinding and
        // replaces the useful stack with panic_cannot_unwind in the macOS crash report.
        record(format_args!("{info}\n{}", Backtrace::force_capture()));
        previous_hook(info);
    }));
    Ok(())
}

pub fn record(message: impl std::fmt::Display) {
    if let Some(log) = LOG.get()
        && let Ok(mut file) = log.lock()
    {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(file, "[{timestamp}] {message}");
        let _ = file.flush();
        let _ = file.sync_data();
    }
}
