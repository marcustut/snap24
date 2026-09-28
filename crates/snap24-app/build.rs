// Compiles the iOS-only UIScene shim into the app binary. No-op elsewhere.
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=ios-shim/S24Scene.m");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        return;
    }

    let simulator = std::env::var("CARGO_CFG_TARGET_ABI").as_deref() == Ok("sim");
    let sdk = if simulator { "iphonesimulator" } else { "iphoneos" };
    let sysroot = Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-path"])
        .output()
        .expect("xcrun --show-sdk-path");
    let sysroot = String::from_utf8(sysroot.stdout).expect("utf8 sysroot");
    let sysroot = sysroot.trim();

    cc::Build::new()
        .file("ios-shim/S24Scene.m")
        .flag("-fobjc-arc")
        .flag(&format!("-isysroot{sysroot}"))
        .compile("s24_scene_shim");
}
