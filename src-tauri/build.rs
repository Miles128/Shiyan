use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    tauri_build::build();
    build_apple_translate_helper();
}

/// Compile the Swift helper that backs the on-device translation engine
/// (Apple Translation framework, macOS 26+).
///
/// The helper is written into `src-tauri/resources/` so the Tauri bundler can
/// ship it inside the .app. When no Swift toolchain is available the build
/// only warns: the engine then reports "unavailable" at runtime and the app
/// falls back to its API engine.
fn build_apple_translate_helper() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = manifest.join("swift/shiyan-translate/main.swift");
    println!("cargo:rerun-if-changed={}", source.display());

    let helper = manifest.join("resources").join("shiyan-translate");
    if source.exists() && !is_fresh(&helper, &source) {
        if let Err(err) = compile_helper(&source, &helper) {
            println!("cargo:warning={err}");
        }
    }
    if helper.exists() {
        // Dev runs (`cargo run`, `tauri dev`) execute the binary from the
        // profile directory — put a copy next to it so the runtime lookup
        // finds the helper there too.
        copy_next_to_binary(&helper);
    }
}

/// True when `out` exists and is newer than `source` (no rebuild needed).
fn is_fresh(out: &Path, source: &Path) -> bool {
    let (Ok(out_meta), Ok(src_meta)) = (std::fs::metadata(out), std::fs::metadata(source)) else {
        return false;
    };
    let (Ok(out_time), Ok(src_time)) = (out_meta.modified(), src_meta.modified()) else {
        return false;
    };
    out_time >= src_time
}

/// Build the helper with the Swift compiler.
fn compile_helper(source: &Path, out: &Path) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // swiftc wants Apple platform triples, not Rust's arch names.
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    let target = format!("{arch}-apple-macos15.0");
    let output = Command::new("xcrun")
        .args(["swiftc", "-O", "-target", &target, "-o"])
        .arg(out)
        .arg(source)
        .output()
        .map_err(|e| format!("shiyan-translate: Swift 工具链不可用（{e}）"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("shiyan-translate: 编译失败：{}", stderr.trim()));
    }
    Ok(())
}

/// Copy the helper next to the built binary.
///
/// `OUT_DIR` is `<target>/<profile>/build/<pkg>-<hash>/out`, so the profile
/// directory (where the app and test binaries land) is three levels up.
fn copy_next_to_binary(helper: &Path) {
    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    let Some(name) = helper.file_name() else {
        return;
    };
    let dest = profile_dir.join(name);
    if dest != helper {
        let _ = std::fs::copy(helper, &dest);
    }
}
