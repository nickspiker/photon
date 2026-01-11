use std::env;
use std::process::Command;

fn main() {
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let allow_release = std::env::var("PHOTON_ALLOW_RELEASE").is_ok();
    if profile == "release" && !allow_release {
        panic!(
            "RELEASE BUILDS DISABLED - Use build-development.sh or build-release!\n READ AGENT.md!"
        );
    }
    let target = env::var("TARGET").unwrap_or_default();

    // Only embed icon when building for Windows
    if target.contains("windows") {
        println!("cargo:rerun-if-changed=photon-messenger.rc");
        println!("cargo:rerun-if-changed=assets/photon-messenger.ico");

        // Try to compile the resource file with windres (MinGW cross-compiler)
        let windres = if target.contains("x86_64") {
            "x86_64-w64-mingw32-windres"
        } else {
            "i686-w64-mingw32-windres"
        };

        let out_dir = env::var("OUT_DIR").unwrap();
        let res_file = format!("{}/photon-messenger.res", out_dir);

        let status = Command::new(windres)
            .args(&["photon-messenger.rc", "-O", "coff", "-o", &res_file])
            .status();

        match status {
            Ok(status) if status.success() => {
                // Link the compiled resource file
                println!("cargo:rustc-link-arg={}", res_file);
                println!("Icon embedded successfully via windres");
            }
            _ => {
                eprintln!("Warning: Failed to embed icon via windres");
                eprintln!("  Icon will not appear in Windows Explorer");
                eprintln!("  Install mingw-w64 tools for icon embedding");
            }
        }
    }

    // Redox: pqc_dilithium tries to build cdylib which requires libc.a
    // We only need rlib, so tell linker to not error on missing libc
    if target.contains("redox") {
        println!("cargo:rustc-link-arg=-Wl,--allow-shlib-undefined");
    }

    // Tell cargo to rerun if icon changes
    println!("cargo:rerun-if-changed=assets/photon-messenger.ico");
    println!("cargo:rerun-if-changed=assets/icon-256.png");
}
