//! Detects the optional `assets/kestrel-mark.svg` sidebar logo. Without it the
//! app embeds `kestrel.png` from the egui app instead.

use std::path::Path;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(kestrel_mark)");
    println!("cargo:rerun-if-changed=assets/kestrel-mark.svg");
    if Path::new("assets/kestrel-mark.svg").is_file() {
        println!("cargo:rustc-cfg=kestrel_mark");
    }
}
