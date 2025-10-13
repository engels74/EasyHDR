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
