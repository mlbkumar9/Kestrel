//! Detects the optional `assets/kestrel-mark.svg` sidebar logo (without it the app embeds
//! `kestrel.png` from the egui app) and embeds `assets/kestrel.ico` as the Windows exe icon.

use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(kestrel_mark)");
    println!("cargo:rerun-if-changed=assets/kestrel-mark.svg");
    println!("cargo:rerun-if-changed=assets/kestrel.ico");
    if Path::new("assets/kestrel-mark.svg").is_file() {
        println!("cargo:rustc-cfg=kestrel_mark");
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").expect("Cargo provides the package version");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/kestrel.ico")
        .set("ProductName", "Kestrel")
        .set("FileDescription", "Kestrel")
        .set("CompanyName", "mlbkumar9")
        .set("LegalCopyright", "Copyright (c) 2025 mlbkumar9")
        .set("FileVersion", &version)
        .set("ProductVersion", &version);
    resource
        .compile()
        .expect("Failed to compile the Kestrel Windows resources");
}
