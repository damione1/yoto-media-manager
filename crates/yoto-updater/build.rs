#[allow(dead_code)]
#[path = "src/version.rs"]
mod version;

fn main() {
    println!("cargo:rerun-if-env-changed=YOTO_APP_VERSION");
    if let Ok(value) = std::env::var("YOTO_APP_VERSION")
        && !value.is_empty()
        && version::parse_release(&value).is_none()
    {
        panic!("YOTO_APP_VERSION must be a release number such as 1.2.3, got {value:?}");
    }
}
