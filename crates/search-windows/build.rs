use std::path::Path;

const RC: &str = r#"1 24 "app.manifest"

100 DIALOGEX 0, 0, 320, 200
STYLE 0x80CA08C0
CAPTION "Filestash Search"
FONT 9, "Segoe UI"
BEGIN
    EDITTEXT 101, 7, 7, 306, 14, 0x0080
    LISTBOX 102, 7, 26, 306, 167, 0x00210100
END
"#;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("app.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    std::fs::copy(&manifest, Path::new(&out_dir).join("app.manifest")).expect("copy app manifest");
    std::fs::write(Path::new(&out_dir).join("app.rc"), RC).expect("write app.rc");
    embed_resource::compile(Path::new(&out_dir).join("app.rc"), embed_resource::NONE)
        .manifest_optional()
        .expect("compile app.rc");
}
