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

    // macOS: embed an Info.plist section carrying NSBluetoothAlwaysUsageDescription.
    // The pairing beacon (docs/pairing-v2.md) creates a CoreBluetooth central on the AddDevice screen;
    // without this key macOS SIGABRTs the process the instant that central powers on. The dev/release
    // builds ship a bare binary (no .app bundle), so the plist rides a __TEXT,__info_plist Mach-O section
    // via ld64's -sectcreate (works under both osxcross and a native toolchain), which is the standard way
    // a command-line tool declares a TCC usage string.
    if target.contains("apple") {
        let plist = format!("{}/macos/Info.plist", env::var("CARGO_MANIFEST_DIR").unwrap());
        println!("cargo:rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{plist}");
        println!("cargo:rerun-if-changed=macos/Info.plist");
        // CoreBluetooth advertiser shim (pairing-beacon new-device role) — ObjC because CBPeripheralManager needs a delegate + run loop. Compiled with ARC; links CoreBluetooth + Foundation. cc honours CC_<target>, so it rides the osxcross clang wrapper under the cross build.
        cc::Build::new()
            .file("macos/photon_ble.m")
            .flag("-fobjc-arc")
            .compile("photon_ble");
        println!("cargo:rustc-link-lib=framework=CoreBluetooth");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rerun-if-changed=macos/photon_ble.m");
    }

    // Tell cargo to rerun if icon or version changes
    println!("cargo:rerun-if-changed=assets/photon-messenger.ico");
    println!("cargo:rerun-if-changed=assets/icon-256.png");
    println!("cargo:rerun-if-changed=v");

    // Embed the git commit (short) + dirty marker — the dev-channel update manifest records which commit each published dev binary was built from; a dirty tree gets "+dirty" and never claims currency.
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    println!(
        "cargo:rustc-env=PHOTON_GIT_COMMIT={}{}",
        commit,
        if dirty { "+dirty" } else { "" }
    );
    println!("cargo:rerun-if-changed=.git/HEAD");

    // The update stamp window's FLOOR (docs/updates.md): the moment THIS binary was built, in eagle-time oscillations. The automatic path accepts a manifest iff floor < manifest_stamp ≤ now — below the floor is a replay/downgrade, above now is forward-dated ("not yet"). The floor advances only by exec'ing into a newer build: it's compiled in, never mutable stored state. Refreshes only when the crate actually rebuilds (build.rs doesn't rerun on a no-op build), so binary identity ⇒ stamp identity.
    println!(
        "cargo:rustc-env=PHOTON_BUILD_STAMP={}",
        vsf::eagle_time_oscillations()
    );
}
