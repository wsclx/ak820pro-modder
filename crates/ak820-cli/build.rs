//! Inherits the Swift runtime rpath from `ak820-audio-reactive` whenever
//! the audio-reactive smoke probe is compiled in (i.e. on macOS).
//!
//! `cargo:rustc-link-arg` from a *library* build script doesn't propagate
//! to dependent binaries — Cargo treats those instructions as scoped to
//! the package that emitted them. So we duplicate the path-detection
//! logic here. See `crates/ak820-audio-reactive/build.rs` for the
//! detailed reasoning; this file deliberately mirrors that one so the
//! two stay easy to keep in sync.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

    let macos = std::env::var("CARGO_CFG_TARGET_OS")
        .map(|os| os == "macos")
        .unwrap_or(false);
    if !macos {
        return;
    }

    // The Swift Concurrency runtime is referenced by Apple frameworks as
    // `/usr/lib/swift/libswift_Concurrency.dylib`. On macOS 13+ that path
    // is *only* in the dyld shared cache — there is no on-disk file at
    // that location. screencapturekit's static lib references the same
    // library as `@rpath/libswift_Concurrency.dylib`.
    //
    // To make `@rpath/...` resolve to the *same* cache entry the system
    // already loads (not a second copy from a CLT/Xcode toolchain on
    // disk), we bake exactly one rpath: `/usr/lib/swift`. dyld then
    // resolves both references to the same shared-cache library, no
    // duplicate-class warnings, no "spurious casting failures".
    //
    // Older macOS (pre-13) doesn't have Concurrency in the shared cache,
    // so on those targets we fall back to an on-disk toolchain path.
    if shared_cache_has_concurrency() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        return;
    }
    let Some(path) = find_swift_concurrency_dir() else {
        println!(
            "cargo:warning=libswift_Concurrency.dylib not found on disk and \
             this host's macOS predates the dyld shared cache shipping it — \
             `ak820 audio` will fail at load time. Install Xcode or the \
             Command-Line Tools, or set DEVELOPER_DIR to a valid Xcode dir."
        );
        return;
    };
    println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
}

fn shared_cache_has_concurrency() -> bool {
    let Ok(out) = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
    else {
        return false;
    };
    let v = String::from_utf8_lossy(&out.stdout);
    let major: u32 = v
        .trim()
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    major >= 13
}

/// Return the first directory on disk that contains
/// `libswift_Concurrency.dylib`. CLT-only systems (most contributors,
/// our CI) live under `/Library/Developer/CommandLineTools/...`; full
/// Xcode installs live under `xcode-select -p`.
fn find_swift_concurrency_dir() -> Option<String> {
    let dev_dir = developer_dir();
    let candidates = [
        "/Library/Developer/CommandLineTools/usr/lib/swift-5.5/macosx".to_string(),
        "/Library/Developer/CommandLineTools/usr/lib/swift/macosx".to_string(),
        format!("{dev_dir}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx"),
        format!("{dev_dir}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx"),
    ];
    candidates.into_iter().find(|p| {
        std::path::Path::new(p)
            .join("libswift_Concurrency.dylib")
            .exists()
    })
}

fn developer_dir() -> String {
    if let Ok(dir) = std::env::var("DEVELOPER_DIR") {
        return dir;
    }
    match std::process::Command::new("xcode-select")
        .arg("-p")
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}
