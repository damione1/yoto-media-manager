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
thread_local! {static CANCELLATION:RefCell<Option<Arc<AtomicBool>>>=const{RefCell::new(None)};}
pub struct OperationGuard;
impl OperationGuard {
    pub fn install(cancel: Arc<AtomicBool>) -> Self {
        CANCELLATION.with(|v| *v.borrow_mut() = Some(cancel));
        Self
    }
}
impl Drop for OperationGuard {
    fn drop(&mut self) {
        CANCELLATION.with(|v| v.borrow_mut().take());
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
    let cancel = CANCELLATION
        .with(|v| v.borrow().clone())
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let finished = Arc::new(AtomicBool::new(false));
    let flag = finished.clone();
    let watcher = std::thread::spawn(move || {
        let start = Instant::now();
        while !flag.load(Ordering::SeqCst) {
            if cancel.load(Ordering::SeqCst) || start.elapsed() >= timeout {
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
pub fn output(mut command: Command, limit: usize, timeout: Duration) -> Result<Output> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    let mut child = spawn(command, timeout)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Failure::Media("No process output".into()))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = vec![];
        stdout
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let status = child.wait().map_err(crate::media_error)?;
    let stdout = reader
        .join()
        .map_err(|_| Failure::Media("Process output reader failed".into()))?
        .map_err(crate::media_error)?;
    if stdout.len() > limit {
        return Err(Failure::Media("Process output exceeds its limit".into()));
    }
    if CANCELLATION.with(|v| {
        v.borrow()
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
    }) {
        return Err(Failure::Cancelled);
    }
    Ok(Output {
        status,
        stdout,
        stderr: vec![],
    })
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
