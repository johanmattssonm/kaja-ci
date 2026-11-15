use std::path::Path;
use std::process::Command;

fn main() {
    // Rebuild when frontend sources or build assets change.
    println!("cargo:rerun-if-changed=../frontend/src");
    println!("cargo:rerun-if-changed=../frontend/Cargo.toml");
    println!("cargo:rerun-if-changed=../frontend/build.sh");
    println!("cargo:rerun-if-changed=../frontend/main.js");
    println!("cargo:rerun-if-changed=../frontend/styles.css");

    let frontend_dir = Path::new("../frontend");
    let status = Command::new("sh")
        .arg("build.sh")
        .current_dir(frontend_dir)
        .status()
        .expect("Failed to run frontend/build.sh");

    if !status.success() {
        panic!("Frontend build failed while running frontend/build.sh");
    }

    // Backend is built by Cargo automatically through the path dependency:
    // kaja-ci-backend = { path = "../backend" }
    tauri_build::build()
}
