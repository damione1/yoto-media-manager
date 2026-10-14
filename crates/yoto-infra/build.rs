// Local builds read YOTO_CLIENT_ID from the workspace `.env` when the variable is not
// already set; CI keeps injecting it through the environment.
use std::path::Path;

fn main() {
    let env_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    println!("cargo:rerun-if-env-changed=YOTO_CLIENT_ID");
    println!("cargo:rerun-if-changed={}", env_file.display());
    if std::env::var_os("YOTO_CLIENT_ID").is_some() {
        return;
    }
    let Ok(contents) = std::fs::read_to_string(&env_file) else {
        return;
    };
    let client_id = contents.lines().find_map(|line| {
        let (key, value) = line.trim().split_once('=')?;
        (key.trim() == "YOTO_CLIENT_ID").then(|| value.trim().trim_matches(['"', '\'']))
    });
    if let Some(id) = client_id.filter(|id| !id.is_empty()) {
        println!("cargo:rustc-env=YOTO_CLIENT_ID={id}");
    }
}
