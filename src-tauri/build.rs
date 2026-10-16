fn main() {
    // The GNU resource compiler (windres) cannot open paths containing
    // non-ASCII characters. Stage the icon in OUT_DIR, which lives in the
    // (ASCII) target directory, so builds work from any checkout location.
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let staged = out.join("app-icon.ico");
    std::fs::copy("icons/icon.ico", &staged).expect("copy icon");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    let windows = tauri_build::WindowsAttributes::new().window_icon_path(staged);
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("tauri build script failed");
}
