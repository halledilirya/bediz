fn main() -> Result<(), Box<dyn std::error::Error>> {
    // tauri-build watches only its config files, not the icon it compiles into the resource.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))?;
    Ok(())
}
