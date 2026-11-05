use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let lake = env::var_os("LAKE").unwrap_or_else(|| "lake".into());
    println!("cargo:rerun-if-env-changed=LAKE");
    let mut files = vec![
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "verification/Console.lean",
        "verification/Evidence.lean",
        "verification/Main.lean",
        "verification/lean-toolchain",
        "verification/lakefile.toml",
        "web/console.js",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    for dir in ["src", "web"] {
        println!("cargo:rerun-if-changed={dir}");
        for entry in fs::read_dir(root.join(dir)).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                files.push(format!("{dir}/{}", entry.file_name().to_str().unwrap()));
            }
        }
    }
    for name in &files {
        println!("cargo:rerun-if-changed={name}");
    }
    assert!(
        Command::new(lake)
            .arg("build")
            .current_dir(root.join("verification"))
            .status()
            .expect("Install the pinned Lean toolchain with elan")
            .success(),
        "Lean verification failed"
    );
    files.push("verification/.lake/build/bin/console_guard".into());
    let pins: BTreeMap<_, _> = files
        .iter()
        .map(|name| {
            (
                name,
                format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            )
        })
        .collect();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let guard = root.join("verification/.lake/build/bin/console_guard");
    // Cargo executables and the checked guard travel together. No source path is
    // needed at runtime. Keep source digests as build provenance.
    fs::copy(
        &guard,
        out.ancestors().nth(3).unwrap().join("console_guard"),
    )
    .unwrap();
    println!(
        "cargo:rustc-env=GUARD_SHA256={:x}",
        Sha256::digest(fs::read(&guard).unwrap())
    );
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("verified.json"),
        serde_json::to_vec(&pins).unwrap(),
    )
    .unwrap();
}
