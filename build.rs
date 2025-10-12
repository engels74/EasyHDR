fn main() {
    // Compile Slint UI files
    slint_build::compile("ui/main.slint").unwrap();
    
    // Embed Windows resources (icon, version info)
    #[cfg(windows)]
    {
        // Note: winres crate will be added when we create actual icon assets
        // For now, we'll set up the basic structure
        // Uncomment when assets are ready:
        // let mut res = winres::WindowsResource::new();
        // res.set_icon("assets/icon.ico");
        // res.set("ProductName", "EasyHDR");
        // res.set("FileDescription", "Automatic HDR for Windows");
        // res.set("CompanyName", "EasyHDR Contributors");
        // res.compile().unwrap();
    }
}

