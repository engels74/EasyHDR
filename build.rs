//! Build script for `EasyHDR`
//!
//! This build script performs three main tasks:
//! 1. Compiles Slint UI files (`ui/main.slint`) into Rust code
//! 2. Embeds version and build metadata (commit SHA) for runtime access
//! 3. On Windows, embeds application resources (icon, version info, manifest) into the executable
//!
//! The Windows resources include:
//! - Application icon (`assets/icon.ico`)
//! - Version information from `Cargo.toml`
//! - Product metadata (name, description, copyright)
//! - Windows manifest for DPI awareness and compatibility

use std::process::Command;

fn main() {
    // Compile Slint UI files
    slint_build::compile("ui/main.slint").unwrap();

    // Embed version information for runtime access
    println!(
        "cargo:rustc-env=CARGO_PKG_VERSION={}",
        env!("CARGO_PKG_VERSION")
    );

    // Try to get the Git commit SHA (short form, 7 characters)
    // This is used as a build identifier in the version display
    let git_commit = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map_or_else(|| "unknown".to_string(), |s| s.trim().to_string());

    println!("cargo:rustc-env=GIT_COMMIT_SHA={git_commit}");

    // Rerun if .git/HEAD changes (to update commit SHA on new commits)
    println!("cargo:rerun-if-changed=.git/HEAD");

    // Embed Windows resources (icon, version info)
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        // ProductName: Full descriptive name shown in file properties
        res.set(
            "ProductName",
            "EasyHDR - Automatic HDR management for Windows",
        );
        // FileDescription: Brief name shown in Windows Task Manager
        res.set("FileDescription", "EasyHDR");
        res.set("CompanyName", "EasyHDR Contributors");
        res.set("LegalCopyright", "Copyright © 2024 EasyHDR Contributors");
        res.set("OriginalFilename", "easyhdr.exe");
        res.set("FileVersion", env!("CARGO_PKG_VERSION"));
        res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
        res.compile().unwrap();
    }
}
