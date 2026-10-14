//! Bounded child-process execution shared by both source providers and FFmpeg.
use std::{
    cell::RefCell,
    io::Read,
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use yoto_domain::*;
thread_local! {static CANCELLATION:RefCell<Vec<Arc<AtomicBool>>>=const{RefCell::new(Vec::new())};}
pub struct OperationGuard;
impl OperationGuard {
    pub fn install(cancel: Arc<AtomicBool>) -> Self {
        Self::install_any(vec![cancel])
    }
    /// Cancel this thread's child processes when any of `flags` is raised.
    pub fn install_any(flags: Vec<Arc<AtomicBool>>) -> Self {
        CANCELLATION.with(|v| *v.borrow_mut() = flags);
        Self
    }
}
impl Drop for OperationGuard {
    fn drop(&mut self) {
        CANCELLATION.with(|v| v.borrow_mut().clear());
    }
}
pub struct ControlledChild {
    child: Child,
    reaped: bool,
    finished: Arc<AtomicBool>,
    watcher: Option<std::thread::JoinHandle<()>>,
}
impl std::ops::Deref for ControlledChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}
impl std::ops::DerefMut for ControlledChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}
impl ControlledChild {
    pub fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let r = self.child.wait();
        if r.is_ok() {
            self.reaped = true;
        }
        self.finish();
        r
    }
    fn finish(&mut self) {
        self.finished.store(true, Ordering::SeqCst);
        if let Some(w) = self.watcher.take() {
            let _ = w.join();
        }
    }
}
impl Drop for ControlledChild {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::SeqCst);
        if !self.reaped {
            kill_group(self.child.id());
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        self.finish();
    }
}
pub fn spawn(mut command: Command, timeout: Duration) -> Result<ControlledChild> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn().map_err(crate::media_error)?;
    let pid = child.id();
    let cancel = CANCELLATION.with(|v| v.borrow().clone());
    let finished = Arc::new(AtomicBool::new(false));
    let flag = finished.clone();
    let watcher = std::thread::spawn(move || {
        let start = Instant::now();
        while !flag.load(Ordering::SeqCst) {
            if cancel.iter().any(|c| c.load(Ordering::SeqCst)) || start.elapsed() >= timeout {
                kill_group(pid);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    Ok(ControlledChild {
        child,
        reaped: false,
        finished,
        watcher: Some(watcher),
    })
}
/// Most of a child's error output kept for diagnosis: its last lines, where tools report
/// why they failed.
pub const STDERR_TAIL: usize = 32_000;
/// Run `command` to completion within `timeout`. Standard output is capped at `limit`
/// bytes; the last [`STDERR_TAIL`] bytes of error output are returned with it.
pub fn output(mut command: Command, limit: usize, timeout: Duration) -> Result<Output> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = spawn(command, timeout)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Failure::Media("No process output".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Failure::Media("No process error output".into()))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = vec![];
        stdout
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let errors = std::thread::spawn(move || read_tail(stderr, STDERR_TAIL));
    let status = child.wait().map_err(crate::media_error)?;
    let stdout = reader
        .join()
        .map_err(|_| Failure::Media("Process output reader failed".into()))?
        .map_err(crate::media_error)?;
    let stderr = errors
        .join()
        .map_err(|_| Failure::Media("Process error reader failed".into()))?;
    if stdout.len() > limit {
        return Err(Failure::Media("Process output exceeds its limit".into()));
    }
    if cancelled() {
        return Err(Failure::Cancelled);
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}
/// Drain `source` completely, keeping only its last `keep` bytes.
fn read_tail(mut source: impl Read, keep: usize) -> Vec<u8> {
    let mut tail = Vec::with_capacity(keep.min(64_000));
    let mut chunk = [0u8; 8192];
    loop {
        match source.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                tail.extend_from_slice(&chunk[..n]);
                if tail.len() > keep * 2 {
                    tail.drain(..tail.len() - keep);
                }
            }
        }
    }
    if tail.len() > keep {
        tail.drain(..tail.len() - keep);
    }
    tail
}
fn cancelled() -> bool {
    CANCELLATION.with(|v| v.borrow().iter().any(|c| c.load(Ordering::SeqCst)))
}
/// Wait before trying again, ending early when the operation is cancelled.
pub fn pause(duration: Duration) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < duration {
        if cancelled() {
            return Err(Failure::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(50).min(duration.saturating_sub(start.elapsed())));
    }
    if cancelled() {
        return Err(Failure::Cancelled);
    }
    Ok(())
}
fn kill_group(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill.exe")
            .creation_flags(0x08000000)
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(test)]
mod tail_tests {
    #[test]
    fn only_the_end_of_long_error_output_is_kept() {
        let text: Vec<u8> = (0..100_000u32).map(|i| b'a' + (i % 26) as u8).collect();
        let tail = super::read_tail(&text[..], 1000);
        assert_eq!(tail, &text[text.len() - 1000..]);
        assert_eq!(super::read_tail(&b"short"[..], 1000), b"short");
    }
}
