//! Bake the Swift runtime rpath into anything that links us with the
//! `capture` feature on macOS.
//!
//! # Why this exists
//!
//! `screencapturekit` (our audio source) is a Swift-bridge crate. The
//! generated static lib references `libswift_Concurrency.dylib`, which
//! lives in different places depending on the user's toolchain:
//!
//! - Full Xcode → `$(xcode-select -p)/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx`
//! - Command-Line Tools only → `/Library/Developer/CommandLineTools/usr/lib/swift-5.5/macosx`
//! - macOS 15+ system → `/usr/lib/swift` (partial; lacks Concurrency on 14.x)
//!
//! screencapturekit's own build script only handles the Xcode layout —
//! it composes `$xcode_path/Toolchains/...` and silently produces an
//! invalid path when `xcode-select -p` returns the CLT prefix. The
//! resulting binary loads at runtime with `dyld: Library not loaded: …
//! libswift_Concurrency.dylib (Reason: no LC_RPATH's found)`.
//!
//! We bake every realistic candidate as an `LC_RPATH`. dyld picks the
//! first one that contains the dylib at runtime; non-existent rpaths
//! cost nothing.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SDKROOT");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

    let capture_on = std::env::var("CARGO_FEATURE_CAPTURE").is_ok();
    let macos = std::env::var("CARGO_CFG_TARGET_OS")
        .map(|os| os == "macos")
        .unwrap_or(false);
    if !capture_on || !macos {
        return;
    }

    // See `ak820-cli/build.rs` for the full reasoning. On macOS 13+ point
    // @rpath at /usr/lib/swift so dyld resolves via the shared cache —
    // same place Apple frameworks load Concurrency from, no duplicates.
    if shared_cache_has_concurrency() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        return;
    }
    let Some(path) = find_swift_concurrency_dir() else {
        println!(
            "cargo:warning=could not locate libswift_Concurrency.dylib; \
             test binaries for ak820-audio-reactive will fail to load."
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
