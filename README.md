# EasyHDR

**Automatic HDR management for Windows**

EasyHDR is a lightweight Windows utility that automatically enables HDR (High Dynamic Range) when you launch your favorite games and applications, then disables it when you close them. No more manual toggling in Windows settings!

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](LICENSE)
[![Windows](https://img.shields.io/badge/Platform-Windows%2010%2F11-blue.svg)](https://www.microsoft.com/windows)
[![Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)

## Features

- 🎮 **Automatic HDR toggling** based on configured applications
- 🖥️ **Native Windows GUI** built with Slint for a modern, responsive interface
- 📊 **System tray integration** with HDR status indicator
- ⚡ **Minimal resource usage** - less than 1% CPU and 50MB RAM
- 💾 **Configuration persistence** - your settings are saved automatically
- 🔄 **Multi-monitor support** - works with multiple HDR displays
- 🚀 **Auto-start capability** - launches automatically with Windows
- 📝 **Comprehensive logging** for troubleshooting

## System Requirements

### Minimum Requirements

- **Operating System**: Windows 10 21H2 (Build 19044) or later
- **Display**: HDR-capable monitor (for HDR functionality)
- **Disk Space**: 50MB free
- **Memory**: 100MB RAM

### Recommended

- **Operating System**: Windows 11 24H2 (Build 26100) or later
- **Display**: HDR-capable monitor with updated drivers
- **Disk Space**: 100MB free
- **Memory**: 200MB RAM

### Supported Windows Versions

- ✅ Windows 10 21H2+ (Build 19044+)
- ✅ Windows 11 21H2+ (Build 22000+)
- ✅ Windows 11 24H2+ (Build 26100+) - Uses new HDR APIs

**Note**: Windows 11 24H2 and later use improved HDR APIs for better performance and reliability.

## Quick Start Guide

### Installation

1. **Download** the latest release from the [Releases](https://github.com/engels74/EasyHDR/releases) page
2. **Extract** `easyhdr.exe` to a folder of your choice (e.g., `C:\Program Files\EasyHDR\`)
3. **Run** `easyhdr.exe` - no installation required!

The application will:
- Create a configuration directory at `%APPDATA%\EasyHDR\`
- Display the main window
- Add an icon to your system tray

### First-Time Setup

1. **Launch EasyHDR** by double-clicking `easyhdr.exe`
2. **Add your applications**:
   - Click the **"Add Application"** button
   - Browse to your game or application's `.exe` file
   - Select it and click **"Open"**
3. **Verify the application** appears in the list with:
   - Application icon
   - Display name
   - Full path
   - Enabled checkbox (checked by default)
4. **Minimize to tray** by clicking the close button (X)

The application will now monitor your configured programs and automatically toggle HDR!

### Basic Usage

#### Adding Applications

**Method 1: File Picker**
1. Click **"Add Application"**
2. Navigate to your game/app folder (e.g., `C:\Games\Cyberpunk 2077\bin\x64\`)
3. Select the `.exe` file
4. Click **"Open"**

**Method 2: Multi-Select**
1. Click **"Add Application"**
2. Hold `Ctrl` or `Shift` to select multiple `.exe` files
3. Click **"Open"** to add them all at once

**Common Application Locations:**
- Steam games: `C:\Program Files (x86)\Steam\steamapps\common\`
- Epic Games: `C:\Program Files\Epic Games\`
- Xbox Game Pass: `C:\Program Files\WindowsApps\` (requires permissions)
- GOG games: `C:\GOG Games\`

#### Removing Applications

1. **Select** an application from the list by clicking on it
2. Click **"Remove Selected"**
3. The application will be removed from monitoring

#### Enabling/Disabling Applications

- **Toggle** the checkbox next to any application to enable or disable HDR monitoring
- Disabled applications remain in your list but won't trigger HDR changes

#### System Tray

- **Left-click** the tray icon to open the main window
- **Right-click** the tray icon for the menu:
  - **Open** - Show the main window
  - **Current HDR State: ON/OFF** - View current HDR status
  - **Exit** - Close the application

**Tray Icon Indicators:**
- 🟢 **Green icon** - HDR is currently enabled
- 🔴 **Red icon** - HDR is currently disabled

## Configuration

### Settings Panel

Access settings from the main window to configure:

- **Auto-start on Windows login** - Launch EasyHDR automatically when you log in
- **Monitoring interval** - How often to check for running processes (500-2000ms)
- **Startup delay** - Delay before monitoring starts (0-10 seconds)
- **Tray notifications** - Show notifications when HDR state changes

### Configuration File

Settings are stored at: `%APPDATA%\EasyHDR\config.json`

**Example configuration:**
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

**Note**: The configuration file is automatically saved when you make changes. Manual editing is not recommended.

## How It Works

1. **Process Monitoring**: EasyHDR monitors running processes every 500-1000ms
2. **Detection**: When a configured application starts, it's detected within 1-2 seconds
3. **HDR Activation**: HDR is enabled globally across all capable displays
4. **HDR Deactivation**: When the last monitored application closes, HDR is disabled
5. **Debouncing**: A 500ms delay prevents rapid toggling if apps restart quickly

### Technical Details

- **Process Detection**: Uses Windows API snapshot enumeration for efficient monitoring
- **HDR Control**: Direct Windows API integration via DisplayConfigSetDeviceInfo
- **Version Detection**: Automatically detects Windows version and uses appropriate HDR APIs
- **Multi-threading**: Background monitoring thread with minimal CPU overhead (<1%)

## Known Limitations

### Process Name Collisions

**Issue**: EasyHDR matches processes by filename only, not full path.

**Impact**: If multiple applications have the same executable name (e.g., `launcher.exe`), HDR will activate for any of them.

**Example**:
- You configure `C:\Games\GameA\launcher.exe`
- Running `C:\Games\GameB\launcher.exe` will also trigger HDR

**Workaround**: Use applications with unique executable names, or disable monitoring for ambiguous entries.

### UWP Applications (Windows Store Apps)

**Issue**: UWP apps run inside `ApplicationFrameHost.exe`, making them difficult to detect individually.

**Impact**: Cannot monitor specific UWP apps (e.g., Minecraft from Microsoft Store).

**Status**: Future enhancement planned (see [Future Extensibility](#future-extensibility))

**Workaround**: Use Win32 versions of applications when available (e.g., Steam, Epic Games, GOG versions).

### Display Limitations

- **HDR Support Required**: Your display must support HDR for the application to function
- **Driver Compatibility**: Outdated display drivers may cause HDR control failures
- **Multi-Monitor**: All HDR-capable displays are toggled together (per-monitor control planned for future)

## Troubleshooting

### HDR Not Toggling

**Symptoms**: Application starts but HDR doesn't enable

**Solutions**:
1. **Verify HDR support**:
   - Open Windows Settings → System → Display
   - Check if "Use HDR" toggle is available
   - If not, your display doesn't support HDR

2. **Update display drivers**:
   - Visit your GPU manufacturer's website (NVIDIA, AMD, Intel)
   - Download and install the latest drivers
   - Restart your computer

3. **Check application is enabled**:
   - Open EasyHDR main window
   - Verify the checkbox next to your application is checked

4. **Verify process name**:
   - Check logs at `%APPDATA%\EasyHDR\app.log`
   - Look for process detection events
   - Ensure the process name matches

### Application Not Detected

**Symptoms**: Application runs but EasyHDR doesn't detect it

**Solutions**:
1. **Check the executable path**:
   - Some games launch through launchers
   - Add the actual game executable, not the launcher
   - Example: For Steam games, add the game's `.exe`, not `steam.exe`

2. **Verify process name**:
   - Open Task Manager while the game is running
   - Find the process under "Details" tab
   - Note the exact process name (without `.exe`)
   - Ensure it matches the configured application

3. **Check monitoring interval**:
   - Detection can take 1-2 seconds
   - Increase monitoring interval if needed (Settings panel)

### Error Messages

#### "Your display doesn't support HDR"

**Cause**: No HDR-capable displays detected

**Solutions**:
- Verify your monitor supports HDR (check manufacturer specifications)
- Ensure HDR is enabled in Windows Settings
- Update display drivers
- Check display cable supports HDR (use DisplayPort 1.4+ or HDMI 2.0+)

#### "Unable to control HDR - check display drivers"

**Cause**: Windows API calls failing, usually due to driver issues

**Solutions**:
- Update GPU drivers to the latest version
- Restart your computer
- Try disabling and re-enabling HDR manually in Windows Settings
- Check Windows Event Viewer for display-related errors

#### "Failed to load or save configuration"

**Cause**: Permission issues or corrupted config file

**Solutions**:
- Ensure `%APPDATA%\EasyHDR\` directory exists and is writable
- Delete `config.json` to reset to defaults
- Run EasyHDR as administrator (not recommended for normal use)
- Check disk space availability

### Performance Issues

#### High CPU Usage

**Expected**: <1% CPU during normal operation

**If higher**:
- Check monitoring interval (increase to 1500-2000ms)
- Verify no other process monitoring tools are conflicting
- Check logs for repeated errors
- Restart EasyHDR

#### High Memory Usage

**Expected**: <50MB RAM during normal operation

**If higher**:
- Check number of configured applications (icons are cached)
- Restart EasyHDR to clear any memory leaks
- Check logs for errors

### Logs and Diagnostics

**Log Location**: `%APPDATA%\EasyHDR\app.log`

**Log Rotation**: Logs are rotated at 5MB, keeping 3 historical files:
- `app.log` - Current log
- `app.log.1` - Previous log
- `app.log.2` - Older log

**Log Levels**:
- `ERROR` - Critical errors requiring attention
- `WARN` - Warnings about recoverable issues
- `INFO` - Normal operation events (default)
- `DEBUG` - Detailed diagnostic information

**Viewing Logs**:
1. Press `Win + R`
2. Type `%APPDATA%\EasyHDR\`
3. Press Enter
4. Open `app.log` with Notepad

**Common Log Entries**:
- `EasyHDR v0.1.0 started` - Application started successfully
- `Process started: cyberpunk2077` - Monitored application detected
- `Toggling HDR: ON` - HDR being enabled
- `HDR enabled for display` - HDR successfully enabled
- `Failed to enable HDR` - HDR control error (check drivers)

## Building from Source

### Prerequisites

- **Rust** 1.70 or later ([Install Rust](https://www.rust-lang.org/tools/install))
- **Windows 10/11** (required for Windows-specific dependencies)
- **Visual Studio Build Tools** (for MSVC toolchain)

### Build Steps

```bash
# Clone the repository
git clone https://github.com/engels74/EasyHDR.git
cd EasyHDR

# Build debug version
cargo build

# Build release version (optimized)
cargo build --release

# Run tests
cargo test

# Run the application
cargo run
```

### Release Build

The release build is optimized for size and performance:
- **Size optimization** (`opt-level = "z"`)
- **Link-time optimization** (LTO enabled)
- **Debug symbols stripped**
- **Typical size**: 2-5MB

Output: `target/release/easyhdr.exe`

## Future Extensibility

Planned features for future releases:

- **Per-Monitor HDR Control** - Toggle HDR on specific displays only
- **UWP Application Support** - Detect and monitor Windows Store apps
- **Focus-Based Toggling** - Enable HDR based on window focus, not just running state
- **HDR Profiles** - Configure brightness, color temperature per application
- **Command-Line Interface** - Scriptable HDR control for automation
- **Update Mechanism** - Automatic update notifications and downloads

## Contributing

Contributions are welcome! Please feel free to submit issues, feature requests, or pull requests.

### Development Guidelines

- Follow Rust best practices and idioms
- Add tests for new functionality
- Update documentation for user-facing changes
- Ensure all tests pass before submitting PRs

## License

This project is licensed under the **GNU Affero General Public License v3.0** (AGPL-3.0).

See [LICENSE](LICENSE) for the full license text.

### What This Means

- ✅ You can use, modify, and distribute this software
- ✅ You can use it for commercial purposes
- ⚠️ You must disclose the source code of any modifications
- ⚠️ You must license derivative works under AGPL-3.0
- ⚠️ Network use counts as distribution (must provide source)

## Acknowledgments

- Built with [Rust](https://www.rust-lang.org/) and [Slint](https://slint.dev/)
- Uses [windows-rs](https://github.com/microsoft/windows-rs) for Windows API integration
- Icon design and UI inspired by modern Windows applications

## Support

- **Issues**: [GitHub Issues](https://github.com/engels74/EasyHDR/issues)
- **Discussions**: [GitHub Discussions](https://github.com/engels74/EasyHDR/discussions)
- **Documentation**: This README and inline code documentation

---

**Made with ❤️ for the HDR gaming community**

