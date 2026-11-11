use std::env;

fn main() {
    println!("cargo:rerun-if-changed=assets/kestrel.ico");

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
