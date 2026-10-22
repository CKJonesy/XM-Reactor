fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let v = |key: &str| std::env::var(key).expect(key);
    // The version resource in app.rc reads these.
    let macros = [
        format!("VER_MAJOR={}", v("CARGO_PKG_VERSION_MAJOR")),
        format!("VER_MINOR={}", v("CARGO_PKG_VERSION_MINOR")),
        format!("VER_PATCH={}", v("CARGO_PKG_VERSION_PATCH")),
        format!("VER_STR=\"{}\"", v("CARGO_PKG_VERSION")),
    ];
    embed_resource::compile("app.rc", macros)
        .manifest_required()
        .unwrap();
}
