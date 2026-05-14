fn main() {
    // Tauri's own build hooks.
    tauri_build::build();

    // Bake the Swift Concurrency rpath on macOS when linking against
    // `ak820-audio-reactive`'s capture feature. See
    // `docs/HANDOFF.md § 6.9g` for the full reasoning and
    // `crates/ak820-cli/build.rs` for the original logic. The three
    // copies (audio crate's tests, CLI bin, this Tauri bin) drift
    // seldom enough that 30-line duplication beats a shared crate
    // for build-script-only logic.
    swift_rpath::emit();
}

mod swift_rpath {
    pub fn emit() {
        println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

        let macos = std::env::var("CARGO_CFG_TARGET_OS")
            .map(|os| os == "macos")
            .unwrap_or(false);
        if !macos {
            return;
        }

        // macOS 13+: resolve @rpath/libswift_Concurrency.dylib via the
        // dyld shared cache (rpath = /usr/lib/swift). Same library Apple
        // frameworks load → single load, no duplicate-class warnings.
        // Older macOS: fall back to a toolchain copy on disk.
        if shared_cache_has_concurrency() {
            println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
            return;
        }
        let Some(path) = find_swift_concurrency_dir() else {
            println!(
                "cargo:warning=libswift_Concurrency.dylib not found on disk and \
                 this macOS predates the shared-cache copy — the bundled app \
                 will fail to launch when audio-reactive is enabled."
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
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            _ => String::new(),
        }
    }
}
