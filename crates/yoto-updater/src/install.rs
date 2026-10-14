use super::*;
use std::{
    fs,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
    process::Command,
};
#[derive(Debug)]
pub struct PreparedUpdate {
    scratch: tempfile::TempDir,
    plan: InstallPlan,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallPlan {
    parent_pid: u32,
    /// The running installation, moved to `backup` once the payload is ready.
    source: PathBuf,
    /// Where the payload goes: `source`, or its renamed sibling when migrating `Yoto.app`.
    destination: PathBuf,
    payload: PathBuf,
    backup: PathBuf,
    platform: Platform,
    version: Version,
}
impl PreparedUpdate {
    /// Start a copy of the current executable outside the installation directory.
    /// The helper waits for the GUI/IPC owner to exit before replacing any files.
    pub fn launch_helper(&self) -> Result<(), UpdateError> {
        let helper = self.scratch.path().join(helper_name());
        let plan = self.scratch.path().join("install.json");
        fs::write(&plan, serde_json::to_vec(&self.plan).map_err(error)?).map_err(error)?;
        // Arm cleanup protection before spawn, never leave a hidden armed helper on failure.
        let marker = self.scratch.path().join("helper-started");
        fs::write(&marker, b"1").map_err(error)?;
        if let Err(failure) = Command::new(helper).arg("--apply-update").arg(plan).spawn() {
            let _ = fs::remove_file(marker);
            return Err(error(failure));
        }
        Ok(())
    }
}
impl Drop for PreparedUpdate {
    fn drop(&mut self) {
        if self.scratch.path().join("helper-started").exists() {
            // TempDir otherwise deletes the executable while the helper is starting.
            self.scratch.disable_cleanup(true);
        }
    }
}
fn helper_name() -> &'static str {
    if cfg!(windows) {
        "yoto-update-helper.exe"
    } else {
        "yoto-update-helper"
    }
}
fn error(e: impl std::fmt::Display) -> UpdateError {
    UpdateError::Install(e.to_string())
}
fn archive_error(e: impl std::fmt::Display) -> UpdateError {
    UpdateError::Archive(e.to_string())
}
/// Folder names an update archive may use for the application, current name first.
fn archive_roots(platform: Platform) -> [String; 2] {
    match platform {
        Platform::MacArm | Platform::MacIntel => {
            [APP_NAME, LEGACY_APP_NAME].map(|name| format!("{name}.app"))
        }
        Platform::Windows => [APP_NAME, LEGACY_APP_NAME].map(str::to_owned),
    }
}
/// Where an update installs the application currently at `source`.
///
/// A macOS `Yoto.app` becomes `YotoMediaManager.app` in the same folder, unless that name
/// is already taken by another copy, which is never overwritten. Every other bundle name
/// (a development build, a copy renamed by hand) is kept. On Windows the folder is kept:
/// the installer's uninstall entry, uninstaller and shortcuts point at it, and the
/// installer itself moves `Programs\Yoto` to `Programs\YotoMediaManager`.
fn target_installation(source: &Path, platform: Platform) -> PathBuf {
    let legacy = format!("{LEGACY_APP_NAME}.app");
    match platform {
        Platform::MacArm | Platform::MacIntel
            if source
                .file_name()
                .is_some_and(|name| name == legacy.as_str()) =>
        {
            let renamed = source.with_file_name(format!("{APP_NAME}.app"));
            if fs::symlink_metadata(&renamed).is_err() {
                renamed
            } else {
                source.to_path_buf()
            }
        }
        Platform::MacArm | Platform::MacIntel | Platform::Windows => source.to_path_buf(),
    }
}
fn installation(platform: Platform) -> Result<PathBuf, UpdateError> {
    let exe = fs::canonicalize(std::env::current_exe().map_err(error)?).map_err(error)?;
    match platform {
        Platform::MacArm | Platform::MacIntel => exe
            .ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                error("Install the release application in a writable folder before updating")
            }),
        Platform::Windows => {
            if exe.file_name().is_none_or(|s| s != "yoto-desktop.exe") {
                return Err(error("Unexpected installation executable"));
            }
            exe.parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| error("Installation folder unavailable"))
        }
    }
}
pub(super) fn prepare(
    bytes: &[u8],
    platform: Platform,
    version: Version,
) -> Result<PreparedUpdate, UpdateError> {
    let source = installation(platform)?;
    let destination = target_installation(&source, platform);
    let parent = source
        .parent()
        .ok_or_else(|| error("Installation parent unavailable"))?;
    let scratch = tempfile::Builder::new()
        .prefix(".yoto-update-")
        .tempdir_in(parent)
        .map_err(error)?;
    let roots = archive_roots(platform);
    let root = match platform {
        Platform::MacArm | Platform::MacIntel => extract_tar(bytes, scratch.path(), &roots)?,
        Platform::Windows => extract_zip(bytes, scratch.path(), &roots)?,
    };
    let payload = scratch.path().join(root);
    validate_payload(&payload, platform)?;
    if platform == Platform::Windows {
        preserve_uninstaller(&source, &payload)?;
    }
    #[cfg(target_os = "macos")]
    if matches!(platform, Platform::MacArm | Platform::MacIntel) {
        let status = Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&payload)
            .status()
            .map_err(error)?;
        if !status.success() {
            return Err(error("Apple code signature verification failed"));
        }
        if team(&source)? != team(&payload)? {
            return Err(error("Apple signing identity changed"));
        }
    }
    let helper = scratch.path().join(helper_name());
    fs::copy(std::env::current_exe().map_err(error)?, helper).map_err(error)?;
    let backup = parent.join(format!(
        ".yoto-backup-{}{}",
        uuid::Uuid::new_v4(),
        if matches!(platform, Platform::MacArm | Platform::MacIntel) {
            ".app"
        } else {
            ""
        }
    ));
    let plan = InstallPlan {
        parent_pid: std::process::id(),
        source,
        destination,
        payload,
        backup,
        platform,
        version,
    };
    Ok(PreparedUpdate { scratch, plan })
}
#[cfg(target_os = "macos")]
fn team(path: &Path) -> Result<String, UpdateError> {
    let output = Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=4"])
        .arg(path)
        .output()
        .map_err(error)?;
    let text = String::from_utf8_lossy(&output.stderr);
    let team = text
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|value| !value.is_empty() && *value != "not set")
        .ok_or_else(|| error("A Developer ID signed application is required"))?;
    Ok(team.into())
}
fn validate_payload(payload: &Path, platform: Platform) -> Result<(), UpdateError> {
    let executable = match platform {
        Platform::MacArm | Platform::MacIntel => payload.join("Contents/MacOS/yoto-desktop"),
        Platform::Windows => payload.join("yoto-desktop.exe"),
    };
    let metadata = fs::symlink_metadata(executable).map_err(error)?;
    if !metadata.is_file() {
        return Err(archive_error("Missing application executable"));
    }
    Ok(())
}
/// The archive's single top-level folder: the first entry picks one of `roots`, every
/// later entry must share it.
fn archive_root<'a>(
    path: &Path,
    roots: &'a [String],
    chosen: &mut Option<&'a str>,
) -> Result<&'a str, UpdateError> {
    let root = match *chosen {
        Some(root) => root,
        None => {
            let first = path.components().next();
            let root = roots
                .iter()
                .find(|root| first == Some(Component::Normal(std::ffi::OsStr::new(root.as_str()))))
                .ok_or_else(|| archive_error("Archive contains an invalid path"))?;
            *chosen = Some(root.as_str());
            root.as_str()
        }
    };
    safe_path(path, root)?;
    Ok(root)
}
fn safe_path(path: &Path, expected_root: &str) -> Result<(), UpdateError> {
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || path.components().next() != Some(Component::Normal(std::ffi::OsStr::new(expected_root)))
    {
        return Err(archive_error("Archive contains an invalid path"));
    }
    Ok(())
}
fn safe_link(path: &Path, target: &Path, expected_root: &str) -> Result<(), UpdateError> {
    let mut parts: Vec<_> = path
        .parent()
        .ok_or_else(|| archive_error("Invalid link"))?
        .components()
        .collect();
    for part in target.components() {
        match part {
            Component::Normal(value) => parts.push(Component::Normal(value)),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.len() <= 1 {
                    return Err(archive_error("Escaping symlink"));
                }
                parts.pop();
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(archive_error("Absolute symlink"));
            }
        }
    }
    if parts.first() != Some(&Component::Normal(std::ffi::OsStr::new(expected_root))) {
        return Err(archive_error("Escaping symlink"));
    }
    Ok(())
}
fn extract_tar<'a>(
    bytes: &[u8],
    destination: &Path,
    roots: &'a [String],
) -> Result<&'a str, UpdateError> {
    let mut chosen = None;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(bytes)));
    let mut total = 0u64;
    let mut links = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (index, entry) in archive.entries().map_err(archive_error)?.enumerate() {
        if index > 250_000 {
            return Err(archive_error("Too many archive entries"));
        }
        let mut entry = entry.map_err(archive_error)?;
        let path = entry.path().map_err(archive_error)?.into_owned();
        let root = archive_root(&path, roots, &mut chosen)?;
        if !seen.insert(path.clone()) {
            return Err(archive_error("Duplicate archive path"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| archive_error("Archive size overflow"))?;
        if total > MAX_EXPANDED {
            return Err(archive_error("Expanded archive is too large"));
        }
        let kind = entry.header().entry_type();
        if kind.is_symlink() {
            let target = entry
                .link_name()
                .map_err(archive_error)?
                .ok_or_else(|| archive_error("Missing symlink target"))?
                .into_owned();
            safe_link(&path, &target, root)?;
            links.push((path, target));
        } else if kind.is_file() || kind.is_dir() {
            // No links exist while files are extracted, so unpack cannot traverse a symlink.
            entry.unpack_in(destination).map_err(archive_error)?;
        } else {
            return Err(archive_error("Unsupported archive entry type"));
        }
    }
    #[cfg(unix)]
    for (path, target) in links {
        let output = destination.join(path);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(archive_error)?;
        }
        std::os::unix::fs::symlink(target, output).map_err(archive_error)?;
    }
    #[cfg(not(unix))]
    if !links.is_empty() {
        return Err(archive_error("Unsupported symlinks"));
    }
    chosen.ok_or_else(|| archive_error("Empty archive"))
}
fn extract_zip<'a>(
    bytes: &[u8],
    destination: &Path,
    roots: &'a [String],
) -> Result<&'a str, UpdateError> {
    let mut chosen = None;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(archive_error)?;
    if archive.len() > 250_000 {
        return Err(archive_error("Too many archive entries"));
    }
    let mut total = 0u64;
    let mut seen = std::collections::BTreeSet::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(archive_error)?;
        let raw_name = std::str::from_utf8(file.name_raw()).map_err(archive_error)?;
        if raw_name.contains('\\') {
            return Err(archive_error(
                "Backslashes are not allowed in update archives",
            ));
        }
        archive_root(Path::new(raw_name), roots, &mut chosen)?;
        let path = file
            .enclosed_name()
            .ok_or_else(|| archive_error("Escaping archive path"))?;
        archive_root(&path, roots, &mut chosen)?;
        for component in path.components() {
            if let Component::Normal(name) = component {
                let name = name.to_string_lossy();
                let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
                if name.contains([':', '\\'])
                    || name.ends_with(['.', ' '])
                    || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    || (stem.len() == 4
                        && (stem.starts_with("COM") || stem.starts_with("LPT"))
                        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
                {
                    return Err(archive_error("Unsafe Windows filename"));
                }
            }
        }
        if !seen.insert(path.clone()) {
            return Err(archive_error("Duplicate archive path"));
        }
        if file.is_symlink() {
            return Err(archive_error("Windows archive contains a symlink"));
        }
        total = total
            .checked_add(file.size())
            .ok_or_else(|| archive_error("Archive size overflow"))?;
        if total > MAX_EXPANDED {
            return Err(archive_error("Expanded archive is too large"));
        }
        let output = destination.join(path);
        if file.is_dir() {
            fs::create_dir_all(output).map_err(archive_error)?;
        } else {
            fs::create_dir_all(
                output
                    .parent()
                    .ok_or_else(|| archive_error("Invalid file path"))?,
            )
            .map_err(archive_error)?;
            let mut writer = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)
                .map_err(archive_error)?;
            let written = std::io::copy(&mut file.by_ref().take(MAX_EXPANDED + 1), &mut writer)
                .map_err(archive_error)?;
            if written > MAX_EXPANDED {
                return Err(archive_error("Expanded file exceeds size bound"));
            }
            writer.sync_all().map_err(archive_error)?;
        }
    }
    chosen.ok_or_else(|| archive_error("Empty archive"))
}
/// Called before GPUI/profile initialization, only by the staged helper executable.
/// Returns true when the helper command was handled.
pub fn apply_pending_update() -> Result<bool, UpdateError> {
    let mut args = std::env::args_os().skip(1);
    if args.next().is_none_or(|arg| arg != "--apply-update") {
        return Ok(false);
    }
    let plan_path = PathBuf::from(args.next().ok_or_else(|| error("Missing update plan"))?);
    if args.next().is_some() {
        return Err(error("Unexpected update arguments"));
    }
    let scratch = fs::canonicalize(
        plan_path
            .parent()
            .ok_or_else(|| error("Invalid plan path"))?,
    )
    .map_err(error)?;
    if scratch
        .file_name()
        .is_none_or(|n| !n.to_string_lossy().starts_with(".yoto-update-"))
        || fs::canonicalize(std::env::current_exe().map_err(error)?).map_err(error)?
            != scratch.join(helper_name())
    {
        return Err(error(
            "Update helper must run from its private staging directory",
        ));
    }
    let mut data = Vec::new();
    fs::File::open(&plan_path)
        .map_err(error)?
        .take(16_001)
        .read_to_end(&mut data)
        .map_err(error)?;
    if data.len() > 16_000 {
        return Err(error("Invalid update plan size"));
    }
    let plan: InstallPlan = serde_json::from_slice(&data).map_err(error)?;
    let parent = scratch
        .parent()
        .ok_or_else(|| error("Invalid staging parent"))?;
    if !valid_paths(&plan, &scratch, parent) {
        return Err(error("Invalid replacement paths"));
    }
    validate_payload(&plan.payload, plan.platform)?;
    wait_for_exit(plan.parent_pid)?;
    if let Err(failure) = replace(&plan) {
        // The GUI has exited: restore/reopen the previous app even on a first rename failure.
        let recovery = [&plan.source, &plan.destination, &plan.backup]
            .into_iter()
            .find(|path| path.exists())
            .unwrap_or(&plan.backup);
        if let Err(relaunch) = launch_application(recovery, plan.platform) {
            return Err(error(format!(
                "{failure}; reopening the previous app failed: {relaunch}"
            )));
        }
        return Err(failure);
    }
    let receipt = CompletedInstall {
        destination: plan.destination.clone(),
        backup: plan.backup.clone(),
    };
    if let Ok(bytes) = serde_json::to_vec(&receipt)
        && fs::write(scratch.join("completed.json"), bytes).is_ok()
    {
        // Earlier updates of a renamed `Yoto.app` recorded the old name.
        cleanup_previous_updates(&scratch, &[&plan.destination, &plan.source]);
    }
    // Keep one previous installation as a recoverable backup, not the user's catalogue.
    Ok(true)
}
/// Checked by the helper before touching anything: every path sits beside the
/// installation, and a rename only ever turns `Yoto.app` into a free
/// `YotoMediaManager.app`.
fn valid_paths(plan: &InstallPlan, scratch: &Path, parent: &Path) -> bool {
    let same_folder = plan.source.parent() == Some(parent)
        && plan.destination.parent() == Some(parent)
        && plan.backup.parent() == Some(parent)
        && plan.payload.parent() == Some(scratch);
    let distinct = !plan.backup.exists()
        && plan.source != plan.backup
        && plan.destination != plan.backup
        && plan.source != scratch
        && plan.destination != scratch;
    let rename = plan.source == plan.destination
        || (matches!(plan.platform, Platform::MacArm | Platform::MacIntel)
            && plan.destination == target_installation(&plan.source, plan.platform));
    same_folder && distinct && rename
}
fn replace(plan: &InstallPlan) -> Result<(), UpdateError> {
    // The new application is fully extracted and verified before the old one moves; the
    // old one stays as the hidden backup, removed by the next completed update.
    fs::rename(&plan.source, &plan.backup).map_err(error)?;
    if let Err(failure) = fs::rename(&plan.payload, &plan.destination) {
        fs::rename(&plan.backup, &plan.source)
            .map_err(|rollback| error(format!("{failure}; rollback: {rollback}")))?;
        return Err(error(failure));
    }
    #[cfg(windows)]
    if plan.platform == Platform::Windows
        && plan.destination.join("unins000.exe").is_file()
        && let Err(failure) = update_uninstall_version(&plan.version)
    {
        rollback(plan)?;
        return Err(failure);
    }
    let result = launch_application(&plan.destination, plan.platform);
    if result.is_err() {
        rollback(plan)?;
        #[cfg(windows)]
        if plan.platform == Platform::Windows
            && plan.source.join("unins000.exe").is_file()
            && let AppVersion::Release(previous) = AppVersion::current()
        {
            let _ = update_uninstall_version(&previous);
        }
        return Err(error(
            "Relaunch failed; the previous application was restored",
        ));
    }
    Ok(())
}
fn launch_application(directory: &Path, platform: Platform) -> Result<(), UpdateError> {
    match platform {
        Platform::MacArm | Platform::MacIntel => {
            let status = Command::new("/usr/bin/open")
                .arg(directory)
                .status()
                .map_err(error)?;
            if !status.success() {
                return Err(error("Unable to launch the application"));
            }
        }
        Platform::Windows => {
            Command::new(directory.join("yoto-desktop.exe"))
                .spawn()
                .map_err(error)?;
        }
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompletedInstall {
    destination: PathBuf,
    backup: PathBuf,
}
fn cleanup_previous_updates(current: &Path, installations: &[&Path]) {
    let Some(parent) = current.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        if folder == current
            || !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".yoto-update-")
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let record = folder.join("completed.json");
        let Ok(reader) = fs::File::open(record) else {
            continue;
        };
        let mut bytes = Vec::new();
        if reader.take(16_001).read_to_end(&mut bytes).is_err() || bytes.len() > 16_000 {
            continue;
        }
        let Ok(previous) = serde_json::from_slice::<CompletedInstall>(&bytes) else {
            continue;
        };
        if !installations.contains(&previous.destination.as_path())
            || previous.backup.parent() != Some(parent)
            || previous
                .backup
                .file_name()
                .is_none_or(|name| !name.to_string_lossy().starts_with(".yoto-backup-"))
        {
            continue;
        }
        if fs::symlink_metadata(&previous.backup)
            .is_ok_and(|kind| kind.is_dir() && !kind.file_type().is_symlink())
        {
            let _ = fs::remove_dir_all(previous.backup);
        }
        let _ = fs::remove_dir_all(folder);
    }
}
fn rollback(plan: &InstallPlan) -> Result<(), UpdateError> {
    fs::rename(&plan.destination, plan.payload.with_extension("failed")).map_err(error)?;
    fs::rename(&plan.backup, &plan.source).map_err(error)
}
fn preserve_uninstaller(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    let mut total = 0u64;
    for entry in fs::read_dir(source).map_err(error)? {
        let entry = entry.map_err(error)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        if name.starts_with("unins")
            && path
                .extension()
                .is_some_and(|ext| matches!(ext.to_str(), Some("exe" | "dat" | "msg")))
        {
            let metadata = fs::symlink_metadata(&path).map_err(error)?;
            total = total
                .checked_add(metadata.len())
                .ok_or_else(|| error("Uninstaller size overflow"))?;
            if !metadata.is_file() || total > 64_000_000 {
                return Err(error("Unsafe existing uninstaller"));
            }
            let output = destination.join(entry.file_name());
            if output.exists() {
                return Err(error(
                    "Update archive must not replace the existing uninstaller",
                ));
            }
            fs::copy(path, output).map_err(error)?;
        }
    }
    Ok(())
}
#[cfg(windows)]
fn update_uninstall_version(version: &Version) -> Result<(), UpdateError> {
    let application_id = include_str!("../../../distribution/windows-app-id.txt").trim();
    let registry_key = format!(
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{{{application_id}}}_is1"
    );
    let status = Command::new("reg.exe")
        .args([
            "add",
            &registry_key,
            "/v",
            "DisplayVersion",
            "/t",
            "REG_SZ",
            "/d",
            &version.to_string(),
            "/f",
            "/reg:64",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(error)?;
    if !status.success() {
        return Err(error("Unable to update installed application version"));
    }
    Ok(())
}
fn wait_for_exit(pid: u32) -> Result<(), UpdateError> {
    if pid == std::process::id() || pid == 0 {
        return Err(error("Invalid parent process"));
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        };
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            let failure = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            if failure == windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER {
                return Ok(());
            }
            return Err(error("Unable to wait for the application to exit"));
        }
        let result = unsafe { WaitForSingleObject(handle, 120_000) };
        unsafe { CloseHandle(handle) };
        if result != WAIT_OBJECT_0 {
            return Err(error("The application is still running"));
        }
    }
    #[cfg(unix)]
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let status = Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(error)?;
            if !status.success() {
                break;
            }
            if std::time::Instant::now() > deadline {
                return Err(error("The application is still running"));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traversal_and_external_symlink_are_rejected() {
        for path in ["../outside", "Yoto.app/../outside", "/Yoto.app/file"] {
            assert!(safe_path(Path::new(path), "Yoto.app").is_err());
        }
        assert!(
            safe_link(
                Path::new("Yoto.app/Contents/link"),
                Path::new("../../../outside"),
                "Yoto.app"
            )
            .is_err()
        );
        assert!(
            safe_link(
                Path::new("Yoto.app/Contents/link"),
                Path::new("/etc/passwd"),
                "Yoto.app"
            )
            .is_err()
        );
        assert!(
            safe_link(
                Path::new("Yoto.app/Contents/Framework/Current"),
                Path::new("Versions/3.14"),
                "Yoto.app"
            )
            .is_ok()
        );
    }
}
#[cfg(test)]
mod safety_tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn unsafe_windows_archives_are_rejected_before_writing() {
        for name in [
            "Yoto/CON",
            "Yoto/AUX.txt",
            "Yoto/audio:secret",
            "Yoto/COM1.txt",
            "Yoto/foo\\bar",
            "Yoto/trailing.",
            "Yoto/../outside",
        ] {
            let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(b"audio").unwrap();
            let mut bytes = archive.finish().unwrap().into_inner();
            if name == "Yoto/foo\\bar" {
                // ZipWriter normalizes backslashes; restore the malicious raw header names.
                for offset in 0..bytes.len().saturating_sub(12) {
                    if &bytes[offset..offset + 12] == b"Yoto/foo/bar" {
                        bytes[offset + 8] = b'\\';
                    }
                }
            }
            let directory = tempfile::tempdir().unwrap();
            let roots = archive_roots(Platform::Windows);
            assert!(
                extract_zip(&bytes, directory.path(), &roots).is_err(),
                "{name}"
            );
            assert!(!directory.path().parent().unwrap().join("outside").exists());
        }
    }
    #[test]
    fn escaping_symlink_and_expanded_tar_bomb_are_rejected() {
        let mut archive = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_mode(0o777);
        header.set_size(0);
        archive
            .append_link(&mut header, "Yoto.app/Contents/escape", "../../../outside")
            .unwrap();
        let raw = archive.into_inner().unwrap();
        let mut writer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        writer.write_all(&raw).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let roots = archive_roots(Platform::MacArm);
        assert!(extract_tar(&writer.finish().unwrap(), directory.path(), &roots).is_err());
        let mut header = tar::Header::new_gnu();
        header.set_path("Yoto.app/huge").unwrap();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(MAX_EXPANDED + 1);
        header.set_cksum();
        let mut writer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        writer.write_all(header.as_bytes()).unwrap();
        assert!(extract_tar(&writer.finish().unwrap(), directory.path(), &roots).is_err());
        assert!(!directory.path().join("Yoto.app/huge").exists());
    }
    #[test]
    fn windows_install_layout_preserves_uninstaller_only() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        fs::create_dir(&old).unwrap();
        fs::create_dir(&new).unwrap();
        for name in [
            "unins000.exe",
            "unins000.dat",
            "unins000.msg",
            "yoto-desktop.exe",
            "settings.json",
        ] {
            fs::write(old.join(name), name).unwrap();
        }
        preserve_uninstaller(&old, &new).unwrap();
        for name in ["unins000.exe", "unins000.dat", "unins000.msg"] {
            assert_eq!(fs::read(new.join(name)).unwrap(), name.as_bytes());
        }
        assert!(!new.join("settings.json").exists());
        assert!(!new.join("yoto-desktop.exe").exists());
    }
    #[cfg(unix)]
    #[test]
    fn launch_io_error_rolls_back_the_original_installation() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("old");
        let payload = root.path().join("new");
        let backup = root.path().join("backup");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&payload).unwrap();
        fs::write(destination.join("original"), "preserved").unwrap();
        let plan = InstallPlan {
            parent_pid: 1,
            source: destination.clone(),
            destination: destination.clone(),
            payload,
            backup: backup.clone(),
            platform: Platform::Windows,
            version: Version::parse("0.2.0").unwrap(),
        };
        assert!(replace(&plan).is_err());
        assert_eq!(
            fs::read_to_string(destination.join("original")).unwrap(),
            "preserved"
        );
        assert!(!backup.exists());
    }
}
#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn payload_move_failure_restores_the_original() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("old");
        let backup = root.path().join("backup");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("original"), "preserved").unwrap();
        let plan = InstallPlan {
            parent_pid: 1,
            source: destination.clone(),
            destination: destination.clone(),
            payload: root.path().join("missing"),
            backup: backup.clone(),
            platform: Platform::Windows,
            version: Version::parse("0.2.0").unwrap(),
        };
        assert!(replace(&plan).is_err());
        assert_eq!(
            fs::read_to_string(destination.join("original")).unwrap(),
            "preserved"
        );
        assert!(!backup.exists());
    }
    #[test]
    fn completed_update_cleanup_keeps_other_installations_and_latest_backup() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("Yoto");
        let current = root.path().join(".yoto-update-current");
        let latest_backup = root.path().join(".yoto-backup-latest");
        fs::create_dir(&current).unwrap();
        fs::create_dir(&latest_backup).unwrap();
        let old = root.path().join(".yoto-update-old");
        let old_backup = root.path().join(".yoto-backup-old");
        fs::create_dir(&old).unwrap();
        fs::create_dir(&old_backup).unwrap();
        fs::write(
            old.join("completed.json"),
            serde_json::to_vec(&CompletedInstall {
                destination: destination.clone(),
                backup: old_backup.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        let other = root.path().join(".yoto-update-other");
        let other_backup = root.path().join(".yoto-backup-other");
        fs::create_dir(&other).unwrap();
        fs::create_dir(&other_backup).unwrap();
        fs::write(
            other.join("completed.json"),
            serde_json::to_vec(&CompletedInstall {
                destination: root.path().join("Other app"),
                backup: other_backup.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        cleanup_previous_updates(&current, &[&destination]);
        assert!(!old.exists());
        assert!(!old_backup.exists());
        assert!(current.exists());
        assert!(latest_backup.exists());
        assert!(other.exists());
        assert!(other_backup.exists());
    }
}
#[cfg(test)]
mod migration_tests {
    use super::*;
    use std::io::Write;
    fn tar_with_root(root: &str) -> Vec<u8> {
        let mut archive = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o755);
        header.set_size(7);
        archive
            .append_data(
                &mut header,
                format!("{root}/Contents/MacOS/yoto-desktop"),
                &b"desktop"[..],
            )
            .unwrap();
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_mode(0o777);
        link.set_size(0);
        archive
            .append_link(
                &mut link,
                format!("{root}/Contents/Resources/current"),
                "../MacOS/yoto-desktop",
            )
            .unwrap();
        let mut writer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        writer.write_all(&archive.into_inner().unwrap()).unwrap();
        writer.finish().unwrap()
    }
    fn zip_with(names: &[&str]) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in names {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(b"binary").unwrap();
        }
        archive.finish().unwrap().into_inner()
    }
    #[test]
    fn archives_with_the_current_or_legacy_root_are_accepted() {
        for root in ["YotoMediaManager.app", "Yoto.app"] {
            let directory = tempfile::tempdir().unwrap();
            let roots = archive_roots(Platform::MacArm);
            let chosen = extract_tar(&tar_with_root(root), directory.path(), &roots).unwrap();
            assert_eq!(chosen, root);
            validate_payload(&directory.path().join(chosen), Platform::MacArm).unwrap();
        }
        for root in ["YotoMediaManager", "Yoto"] {
            let directory = tempfile::tempdir().unwrap();
            let roots = archive_roots(Platform::Windows);
            let bytes = zip_with(&[&format!("{root}/yoto-desktop.exe")]);
            let chosen = extract_zip(&bytes, directory.path(), &roots).unwrap();
            assert_eq!(chosen, root);
            validate_payload(&directory.path().join(chosen), Platform::Windows).unwrap();
        }
    }
    #[test]
    fn archives_with_another_or_mixed_root_are_rejected() {
        let roots = archive_roots(Platform::MacArm);
        for root in ["Other.app", "YotoMediaManager Dev.app", "YotoMediaManager"] {
            let directory = tempfile::tempdir().unwrap();
            assert!(extract_tar(&tar_with_root(root), directory.path(), &roots).is_err());
        }
        let roots = archive_roots(Platform::Windows);
        for names in [
            &["YotoMediaManager/yoto-desktop.exe", "Yoto/tools/ffmpeg.exe"][..],
            &["Yoto/yoto-desktop.exe", "YotoMediaManager/tools/ffmpeg.exe"][..],
            &["Other/yoto-desktop.exe"][..],
        ] {
            let directory = tempfile::tempdir().unwrap();
            assert!(extract_zip(&zip_with(names), directory.path(), &roots).is_err());
        }
        // A safe root does not excuse an escaping symlink under it.
        let mut archive = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        archive
            .append_link(&mut header, "YotoMediaManager.app/escape", "../../outside")
            .unwrap();
        let mut writer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        writer.write_all(&archive.into_inner().unwrap()).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let roots = archive_roots(Platform::MacArm);
        assert!(extract_tar(&writer.finish().unwrap(), directory.path(), &roots).is_err());
    }
    #[test]
    fn legacy_mac_bundle_is_renamed_only_when_the_new_name_is_free() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("Yoto.app");
        let renamed = root.path().join("YotoMediaManager.app");
        for platform in [Platform::MacArm, Platform::MacIntel] {
            assert_eq!(target_installation(&legacy, platform), renamed);
            assert_eq!(target_installation(&renamed, platform), renamed);
            for kept in ["YotoMediaManager Dev.app", "Yoto Dev.app", "Custom.app"] {
                let path = root.path().join(kept);
                assert_eq!(target_installation(&path, platform), path);
            }
        }
        fs::create_dir(&renamed).unwrap();
        assert_eq!(target_installation(&legacy, Platform::MacArm), legacy);
        // Windows keeps its folder: the installer owns the move and its uninstall entry.
        let folder = root.path().join("Yoto");
        assert_eq!(target_installation(&folder, Platform::Windows), folder);
    }
    fn plan(root: &Path, source: &str, destination: &str, platform: Platform) -> InstallPlan {
        InstallPlan {
            parent_pid: 1,
            source: root.join(source),
            destination: root.join(destination),
            payload: root.join(".yoto-update-test").join("YotoMediaManager.app"),
            backup: root.join(".yoto-backup-test.app"),
            platform,
            version: Version::parse("0.2.0").unwrap(),
        }
    }
    #[test]
    fn helper_accepts_only_the_legacy_to_current_rename() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let scratch = root.join(".yoto-update-test");
        fs::create_dir(&scratch).unwrap();
        fs::create_dir(root.join("Yoto.app")).unwrap();
        let migration = plan(root, "Yoto.app", "YotoMediaManager.app", Platform::MacArm);
        assert!(valid_paths(&migration, &scratch, root));
        assert!(valid_paths(
            &plan(root, "Yoto.app", "Yoto.app", Platform::MacArm),
            &scratch,
            root
        ));
        for invalid in [
            plan(root, "Yoto.app", "Other.app", Platform::MacArm),
            plan(root, "Custom.app", "YotoMediaManager.app", Platform::MacArm),
            plan(root, "Yoto", "YotoMediaManager", Platform::Windows),
            plan(root, "Yoto.app", "YotoMediaManager.app", Platform::Windows),
        ] {
            assert!(!valid_paths(&invalid, &scratch, root));
        }
        let mut outside = plan(root, "Yoto.app", "YotoMediaManager.app", Platform::MacArm);
        outside.destination = root.join("nested/YotoMediaManager.app");
        assert!(!valid_paths(&outside, &scratch, root));
        // Another copy already holds the new name: nothing may overwrite it.
        fs::create_dir(root.join("YotoMediaManager.app")).unwrap();
        assert!(!valid_paths(&migration, &scratch, root));
    }
    #[cfg(unix)]
    fn installation_with(directory: &Path, marker: &str, launches: bool) {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(directory).unwrap();
        fs::write(directory.join("marker"), marker).unwrap();
        let launcher = directory.join("yoto-desktop.exe");
        if launches {
            fs::write(&launcher, "#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
        } else {
            fs::write(&launcher, "not executable").unwrap();
        }
    }
    // `replace` is the same for both platforms; the Windows launcher (a spawned executable)
    // lets the test stand in a script instead of asking Finder to open a bundle.
    #[cfg(unix)]
    #[test]
    fn renamed_install_puts_the_new_app_in_place_and_backs_up_the_old_one() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let plan = plan(root, "Yoto.app", "YotoMediaManager.app", Platform::Windows);
        installation_with(&plan.source, "old", true);
        installation_with(&plan.payload, "new", true);
        replace(&plan).unwrap();
        assert!(!plan.source.exists());
        assert_eq!(
            fs::read_to_string(plan.destination.join("marker")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(plan.backup.join("marker")).unwrap(),
            "old"
        );
        // A later update finds the receipt that names the old bundle and removes its backup.
        let scratch = root.join(".yoto-update-earlier");
        fs::create_dir(&scratch).unwrap();
        fs::write(
            scratch.join("completed.json"),
            serde_json::to_vec(&CompletedInstall {
                destination: plan.source.clone(),
                backup: plan.backup.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        cleanup_previous_updates(
            &root.join(".yoto-update-test"),
            &[&plan.destination, &root.join("Yoto.app")],
        );
        assert!(!plan.backup.exists());
        assert!(!scratch.exists());
        assert!(plan.destination.exists());
    }
    #[cfg(unix)]
    #[test]
    fn failed_relaunch_after_rename_restores_the_old_name() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let plan = plan(root, "Yoto.app", "YotoMediaManager.app", Platform::Windows);
        installation_with(&plan.source, "old", true);
        installation_with(&plan.payload, "new", false);
        assert!(replace(&plan).is_err());
        assert_eq!(
            fs::read_to_string(plan.source.join("marker")).unwrap(),
            "old"
        );
        assert!(!plan.destination.exists());
        assert!(!plan.backup.exists());
    }
    #[test]
    fn failed_payload_move_after_rename_restores_the_old_name() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let plan = plan(root, "Yoto.app", "YotoMediaManager.app", Platform::Windows);
        fs::create_dir(&plan.source).unwrap();
        fs::write(plan.source.join("marker"), "old").unwrap();
        assert!(replace(&plan).is_err());
        assert_eq!(
            fs::read_to_string(plan.source.join("marker")).unwrap(),
            "old"
        );
        assert!(!plan.destination.exists());
        assert!(!plan.backup.exists());
    }
}
