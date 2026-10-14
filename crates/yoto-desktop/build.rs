#[allow(dead_code)]
#[path = "../yoto-updater/src/version.rs"]
mod version;

fn main() {
    println!("cargo:rerun-if-env-changed=YOTO_APP_VERSION");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Explorer and the uninstaller show these; a build without a release version is a
        // nightly, numbered 0.0.0 like its macOS bundle.
        let release = std::env::var("YOTO_APP_VERSION")
            .ok()
            .and_then(|value| version::parse_release(&value));
        let (major, minor, patch) = release.unwrap_or((0, 0, 0));
        let display = match release {
            Some(_) => format!("{major}.{minor}.{patch}"),
            None => version::NIGHTLY.to_owned(),
        };
        let number = (major << 48) | (minor << 32) | (patch << 16);
        winresource::WindowsResource::new()
            .set_icon("../../assets/branding/app-icon.ico")
            .set("ProductName", "YotoMediaManager")
            .set("FileDescription", "YotoMediaManager")
            .set("ProductVersion", &display)
            .set("FileVersion", &display)
            .set_version_info(winresource::VersionInfo::PRODUCTVERSION, number)
            .set_version_info(winresource::VersionInfo::FILEVERSION, number)
            .compile()
            .expect("Compile Windows application icon");
    }
}
