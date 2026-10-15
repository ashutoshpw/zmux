use std::{env, path::PathBuf, process::Command};

fn run(command: &mut Command) {
    let status = command.status().expect("cannot execute local C toolchain");
    assert!(status.success(), "local C shim command failed: {command:?}");
}

fn main() {
    println!("cargo:rerun-if-env-changed=ZMUX_GHOSTTY_ARTIFACT_ROOT");
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    if env::var_os("CARGO_FEATURE_LINKED").is_none() {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let artifact = env::var_os("ZMUX_GHOSTTY_ARTIFACT_ROOT").expect(
        "ghostty requires explicit ZMUX_GHOSTTY_ARTIFACT_ROOT; no download/discovery fallback",
    );
    let target = env::var("TARGET").unwrap();
    let output = Command::new("python3")
        .arg(root.join("tools/terminal-engine-artifacts/accept-linkage.py"))
        .args(["--target", &target, "--root"])
        .arg(artifact)
        .output()
        .expect("cannot run retained artifact acceptance");
    assert!(
        output.status.success(),
        "artifact rejected: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let accepted: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("invalid artifact receipt");
    for path in accepted["inputs"].as_array().unwrap() {
        println!("cargo:rerun-if-changed={}", path.as_str().unwrap());
    }
    for name in [
        "accept-linkage.py",
        "linkage-candidates.json",
        "verify.py",
        "darwin_platform.py",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("tools/terminal-engine-artifacts")
                .join(name)
                .display()
        );
    }
    let source = root.join("crates/ghostty-vt/c/owner.c");
    println!("cargo:rerun-if-changed={}", source.display());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let object = out.join("zmux-vt.o");
    let mut cc = Command::new(env::var_os("CC").unwrap_or_else(|| "cc".into()));
    cc.args(["-std=c11", "-Werror", "-fPIC", "-I"])
        .arg(accepted["include"].as_str().unwrap());
    if target.ends_with("-apple-darwin") {
        assert!(
            matches!(
                env::var("MACOSX_DEPLOYMENT_TARGET").as_deref(),
                Ok("14.0" | "14.0.0")
            ),
            "linked Darwin consumers require explicit MACOSX_DEPLOYMENT_TARGET=14.0"
        );
        cc.args([
            "-arch",
            if target.starts_with("aarch64") {
                "arm64"
            } else {
                "x86_64"
            },
            "-mmacosx-version-min=14.0",
        ]);
    } else {
        assert_eq!(
            target,
            env::var("HOST").unwrap(),
            "cross-compilation needs an explicit reviewed C toolchain"
        );
    }
    run(cc.arg("-c").arg(source).arg("-o").arg(&object));
    run(
        Command::new(env::var_os("AR").unwrap_or_else(|| "ar".into()))
            .arg("crs")
            .arg(out.join("libzmux-vt.a"))
            .arg(object),
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!(
        "cargo:rustc-link-search=native={}",
        accepted["library_directory"].as_str().unwrap()
    );
    println!("cargo:rustc-link-lib=static=zmux-vt");
    println!("cargo:rustc-link-lib=static=ghostty-vt");
}
