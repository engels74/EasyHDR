//! Build script for `EasyHDR`
//!
//! This build script performs two main tasks:
//! 1. Compiles Slint UI files (`ui/main.slint`) into Rust code
//! 2. On Windows, embeds application resources (icon, version info, manifest) into the executable
//!
//! The Windows resources include:
//! - Application icon (`assets/icon.ico`)
//! - Version information from `Cargo.toml`
//! - Product metadata (name, description, copyright)
//! - Windows manifest for DPI awareness and compatibility

fn main() {
    // Compile Slint UI files
    slint_build::compile("ui/main.slint").unwrap();

    // Embed Windows resources (icon, version info)
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "EasyHDR");
        res.set("FileDescription", "Automatic HDR for Windows");
        res.set("CompanyName", "EasyHDR Contributors");
        res.set("LegalCopyright", "Copyright © 2024 EasyHDR Contributors");
        res.set("OriginalFilename", "easyhdr.exe");
        res.set("FileVersion", env!("CARGO_PKG_VERSION"));
        res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
        res.compile().unwrap();
    }
}
