# EasyHDR

Automatic HDR management for Windows - automatically enable HDR when your favorite games and applications launch, and disable it when they close.

## Overview

EasyHDR is a Windows-only system utility that automatically manages HDR (High Dynamic Range) display settings based on running applications. It monitors user-configured executables and enables HDR when they launch, then disables HDR when they close.

## Features

- **Automatic HDR toggling** based on configured applications
- **Native Windows GUI** built with Slint
- **System tray integration** with HDR status indicator
- **Process monitoring** with minimal CPU overhead (<1%)
- **Configuration persistence** in `%APPDATA%\EasyHDR\config.json`
- **Support for both legacy and modern Windows HDR APIs**
- **Auto-start on Windows login** (optional)

## System Requirements

### Minimum
- Windows 10 21H2 (build 19044) or later
- HDR-capable display (for HDR functionality)
- 50MB free disk space
- 100MB RAM

### Recommended
- Windows 11 24H2 (build 26100) or later
- HDR-capable display with updated drivers
- 100MB free disk space
- 200MB RAM

## Installation

1. Download the latest release from the [Releases](https://github.com/engels74/EasyHDR/releases) page
2. Extract the executable to a location of your choice
3. Run `easyhdr.exe`
4. The application will create its configuration directory automatically at `%APPDATA%\EasyHDR`

No installer is required - EasyHDR is a portable executable.

## Usage

### Adding Applications

1. Click the "Add Application" button in the main window
2. Browse to the executable (.exe) file of the application you want to monitor
3. The application will be added to the list with HDR monitoring enabled by default

### Managing Applications

- **Enable/Disable monitoring:** Use the checkbox next to each application
- **Remove application:** Select the application and click "Remove Selected"
- **View status:** The HDR status indicator shows whether HDR is currently ON or OFF

### System Tray

- **Left-click** the tray icon to open the main window
- **Right-click** the tray icon for the context menu:
  - Open: Restore the main window
  - Current HDR State: Shows current HDR status (informational)
  - Exit: Close the application

### Settings

Configure application behavior in the Settings panel:
- **Auto-start on Windows login:** Automatically start EasyHDR when Windows starts
- **Monitoring interval:** How often to check for process changes (500-2000ms)
- **Startup delay:** Delay before starting monitoring to avoid boot race conditions (0-10 seconds)
- **Tray notifications:** Show notifications when HDR state changes

## Known Limitations

### Process Name Collisions

**Important:** The process monitor matches applications by their executable filename only (without path or extension). This means:

- If you configure `game.exe` to trigger HDR
- **Any** process named `game.exe` will trigger HDR, regardless of its full path
- This includes both `C:\Games\Game1\game.exe` and `D:\OtherGames\game.exe`

**Workaround:** Ensure that the applications you want to monitor have unique executable names. If you have multiple applications with the same executable name, consider renaming one of them or using a different approach.

**Technical Reason:** The Windows process enumeration API provides only the process name, not the full executable path, when listing running processes. This is a limitation of the current implementation.

### UWP Applications

EasyHDR currently does not support UWP (Universal Windows Platform) applications from the Microsoft Store. Only traditional Win32 executables (.exe files) are supported.

### Display Disconnection

If a display is disconnected while EasyHDR is running:
- The application will continue to operate normally
- HDR toggle operations will fail for the disconnected display but succeed for others
- Errors will be logged but the application will continue monitoring

### Configuration File Deletion

If the configuration file (`%APPDATA%\EasyHDR\config.json`) is deleted while the application is running:
- The application will continue to operate with the in-memory configuration
- Changes will not be persisted to disk until the file can be recreated
- A warning will be logged when save operations fail
- The configuration will be lost when the application is restarted

## Troubleshooting

### HDR doesn't toggle when my application starts

1. **Check if the application is enabled:** Ensure the checkbox next to the application is checked
2. **Verify the process name:** The process name shown in EasyHDR must match the actual running process
3. **Check the logs:** Look at `%APPDATA%\EasyHDR\app.log` for error messages
4. **Verify HDR support:** Ensure your display supports HDR and drivers are up to date

### "Your display doesn't support HDR" error

- Your display hardware does not support HDR
- Check your display specifications and ensure HDR is enabled in Windows Settings
- Update your display drivers to the latest version

### "Unable to control HDR - check display drivers" error

- Your display drivers may be outdated or incompatible
- Update your graphics drivers to the latest version
- Restart your computer after updating drivers

### Application doesn't start

- Ensure you're running Windows 10 21H2 (build 19044) or later
- Check that you have the required Visual C++ redistributables installed
- Run the application as administrator (though this shouldn't be necessary)

### High CPU usage

- Check the monitoring interval in Settings - increase it if needed (default is 1000ms)
- Normal CPU usage should be less than 1% on modern systems
- Check the logs for errors that might indicate a problem

## Configuration File

The configuration is stored in JSON format at `%APPDATA%\EasyHDR\config.json`. You can manually edit this file if needed, but it's recommended to use the GUI.

Example configuration:
```json
{
  "monitored_apps": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "display_name": "Cyberpunk 2077",
      "exe_path": "C:\\Games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe",
      "process_name": "cyberpunk2077",
      "enabled": true
    }
  ],
  "preferences": {
    "auto_start": true,
    "monitoring_interval_ms": 1000,
    "startup_delay_ms": 3000,
    "show_tray_notifications": true
  },
  "window_state": {
    "x": 100,
    "y": 100,
    "width": 600,
    "height": 500
  }
}
```

## Logging

Logs are stored at `%APPDATA%\EasyHDR\app.log`. The log file is automatically rotated when it reaches 5MB, keeping up to 3 historical files.

Log levels:
- **ERROR:** Critical errors that may affect functionality
- **WARN:** Warnings about potential issues
- **INFO:** General information about application activity
- **DEBUG:** Detailed diagnostic information (not shown by default)

## Building from Source

### Prerequisites
- Rust 1.70 or later
- Windows 10 SDK
- Visual Studio Build Tools

### Build Commands
```bash
# Build the project
cargo build

# Build release version
cargo build --release

# Run the application
cargo run

# Run tests
cargo test

# Check code without building
cargo check

# Format code
cargo fmt

# Run linter
cargo clippy
```

The release binary will be located at `target/release/easyhdr.exe`.

## License

EasyHDR is licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).

See [LICENSE](LICENSE) for the full license text.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## Acknowledgments

- Built with [Rust](https://www.rust-lang.org/)
- GUI framework: [Slint](https://slint.dev/)
- Windows API bindings: [windows-rs](https://github.com/microsoft/windows-rs)

## Support

For issues, questions, or feature requests, please use the [GitHub Issues](https://github.com/engels74/EasyHDR/issues) page.

