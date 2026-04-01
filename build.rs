use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/package.json");
    println!("cargo:rerun-if-changed=frontend/rspack.config.js");
    println!("cargo:rerun-if-changed=frontend/postcss.config.mjs");
    println!("cargo:rerun-if-changed=frontend/tsconfig.json");

    let frontend_dir = std::path::Path::new("frontend");

    // Install deps if node_modules missing
    if !frontend_dir.join("node_modules").exists() {
        let status = Command::new("npm")
            .arg("install")
            .current_dir(frontend_dir)
            .status()
            .expect("npm install failed — is Node.js installed?");
        assert!(status.success(), "npm install exited with {status}");
    }

    let status = Command::new("npx")
        .args(["rspack", "build"])
        .current_dir(frontend_dir)
        .status()
        .expect("rspack build failed — is Node.js installed?");
    assert!(status.success(), "rspack build exited with {status}");
}
