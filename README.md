# 🐍 RazerLinux

A userspace Razer mouse configuration tool for Linux. No kernel drivers required!

![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)
![License](https://img.shields.io/badge/license-GPLv3-blue)
![Platform](https://img.shields.io/badge/platform-Linux-green)

## ⚠️ Disclaimer & Safety

**USE AT YOUR OWN RISK.** This software interacts with low-level input devices (evdev/uinput) and USB HID. While we strive for stability, bugs can occur that may temporarily freeze your mouse or keyboard input.

### We Are Not Responsible For:
- System freezes or input lockups
- Data loss from unexpected behavior
- Any damage to your hardware or software
- Lost productivity or frustration

### 🚨 If Your System Freezes (Mouse/Keyboard Unresponsive)

**Don't panic!** You can recover without a hard reboot:

1. **Switch to a TTY console**: Press `Ctrl+Alt+F1` (or F2, F3, etc.)
2. **Log in** with your username and password
3. **Kill the process**:
   ```bash
   sudo pkill razerlinux
   ```
4. **Return to desktop**: Press `Ctrl+Alt+F7` (or whichever TTY your desktop is on)

**Alternative recovery methods:**
- SSH from another computer: `ssh user@your-machine` then `sudo pkill razerlinux`
- If you have a secondary keyboard/mouse, use that to kill the app

### Known Potential Issues
- **Autoscroll overlay** can cause X11 flooding if misconfigured (we've added throttling)
- **evdev grab** takes exclusive control of the mouse - if the app crashes, you may lose input temporarily
- **Running as root** is required for evdev/uinput but carries inherent risks

---

## Features

### Core Features
- **Direct USB HID Communication** - Talks directly to your Razer mouse via hidapi, no kernel driver needed
- **DPI Configuration** - Read and set DPI (100-16000) with independent X/Y axis control
- **Profile Management** - Save and load configuration profiles to TOML files
- **Modern GUI** - Clean Qt-like interface built with Slint
- **Lightweight** - Pure Rust, minimal dependencies

### Button Remapping
- **Software Remapping** - evdev grab + uinput virtual device
- **Modifier Combos** - Combine any button with Ctrl/Alt/Shift/Meta
- **Button Learning** - Click "🎯 Learn Button" to capture any mouse button
- **Side Panel Support** - Automatic Driver Mode switching for Naga Trinity 12-button panel

### Windows-Style Autoscroll ✨ NEW
- **Middle-Click Autoscroll** - Hold middle mouse button to enable continuous scrolling
- **Visual Indicator** - X11 overlay shows anchor point with directional arrows
- **Distance-Based Speed** - Scroll speed increases as you move further from the anchor point
- **Free Cursor Movement** - Mouse cursor moves freely while autoscrolling (like Windows)
- **Click-Through Overlay** - Indicator doesn't interfere with mouse input

## Supported Devices

| Device | Status |
|--------|--------|
| Razer Naga Trinity | ✅ Tested |
| Other Razer Mice | 🔧 Planned |

## Supported Linux Distributions

RazerLinux is a userspace app (hidapi + udev) and should run on most modern Linux distros.

| Distro | Status | Notes |
|--------|--------|-------|
| openSUSE Tumbleweed | ✅ Tested | Primary dev target |
| Fedora | 🟡 Expected to work | Install `hidapi-devel` + `systemd-devel` |
| Ubuntu/Debian | 🟡 Expected to work | Install `libhidapi-dev` + `libudev-dev` |
| Arch/Manjaro | 🟡 Expected to work | Install `hidapi` + `systemd-libs` |

## Screenshots

*Coming soon*

## Installation

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- libhidapi development files
- udev (for device permissions)
- uinput (kernel module + access to `/dev/uinput`) for software remapping

#### openSUSE
```bash
sudo zypper install libhidapi-devel systemd-devel
```

#### Ubuntu/Debian
```bash
sudo apt install libhidapi-dev libudev-dev
```

#### Fedora
```bash
sudo dnf install hidapi-devel systemd-devel
```

### Building from Source

```bash
git clone https://github.com/aleksandarmilacic/razerlinux.git
cd razerlinux
cargo build --release
```

### udev Rules (Required)

To access the device without root, install the udev rules:

```bash
sudo cp config/99-razermouse.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

Then **unplug and replug** your mouse.

## Usage

### Running the GUI

```bash
cargo run --release
```

Or after building:
```bash
./target/release/razerlinux
```

### Features

#### DPI Control
- Adjust DPI from 100 to 16000
- Independent X and Y axis control
- Quick preset buttons (400, 800, 1600, 3200, 6400)

#### Button Remapping (Software)
- Requires `/dev/uinput` access; ensure the uinput module is loaded.
- **Side button support**: When remapping is enabled, RazerLinux automatically switches the Naga Trinity to "Driver Mode", which makes side buttons (1-12) send keyboard key events.
- Works for all mouse buttons (left/right/middle/side/extra) and side panel buttons as sources.
- In the Remapping panel: disable remapping, click "🎯 Learn Button", press the desired button to capture source.
- Set a target code (presets coming soon) and optional modifiers (Ctrl/Alt/Shift/Meta), then click Add.
- Enable remapping to start the virtual device; mappings persist in profiles.
- When remapping is disabled, Normal Mode is restored.

#### Windows-Style Autoscroll
Enable the "Windows Autoscroll" checkbox in the Remapping panel to get Windows-like middle-click scrolling:

1. **Enable Remapping** - Turn on button remapping first
2. **Check "Windows Autoscroll"** - Enables the autoscroll feature
3. **Middle-Click and Hold** - A visual indicator appears at the anchor point
4. **Move Mouse** - Cursor moves freely; scrolling happens based on distance from anchor
5. **Release or Click** - Any mouse button click exits autoscroll mode

**Behavior:**
- **Scroll Threshold**: 20 pixels - no scrolling until you move this far from anchor
- **Speed**: Proportional to distance from anchor point (further = faster)
- **Direction Arrows**: Visual indicator shows active scroll direction
- **Click-Through**: The overlay window doesn't intercept mouse clicks

#### Profile Management
- Save current settings to named profiles
- Load profiles to quickly switch configurations
- Profiles stored in `~/.config/razerlinux/profiles/`

## Configuration

Profiles are stored as TOML files in `~/.config/razerlinux/profiles/`:

```toml
name = "Gaming"
description = "High DPI gaming profile"

[dpi]
x = 1600
y = 1600
linked = true

polling_rate = 1000
brightness = 255

[remap]
enabled = true
autoscroll = true  # Enable Windows-style middle-click autoscroll

[[remap.mappings]]
source = 2          # KEY_1 from side button 1
target = 30         # KEY_A
ctrl = false
alt = false
shift = false
meta = false

[[remap.mappings]]
source = 3          # KEY_2 from side button 2
target = 48         # KEY_B
ctrl = true         # With Ctrl modifier
alt = false
shift = false
meta = false
```

## Architecture

```
razerlinux/
├── src/
│   ├── main.rs       # GUI application entry point & callback wiring
│   ├── device.rs     # USB HID device communication (DPI, mode switching)
│   ├── protocol.rs   # Razer USB protocol implementation (90-byte reports)
│   ├── profile.rs    # Profile save/load management (TOML format)
│   ├── remap.rs      # evdev/uinput software remapper + autoscroll logic
│   ├── overlay.rs    # X11 autoscroll visual indicator (with XShape)
│   └── hidpoll.rs    # Background HID polling for DPI updates
├── ui/
│   └── main.slint    # Slint GUI definition
├── config/
│   └── 99-razermouse.rules  # udev rules for permissions
└── docs/
    └── PROJECT_PLAN.md      # Detailed project documentation
```

### USB Protocol

RazerLinux implements the Razer USB HID protocol:
- 90-byte feature reports
- CRC verification (XOR of bytes 2-87)
- Transaction ID 0xFF for older devices (Naga Trinity)
- Support for DPI, polling rate, and device queries

## Roadmap

### Completed ✅
- [x] USB HID protocol implementation (90-byte feature reports, CRC)
- [x] DPI read/write (100-16000, independent X/Y)
- [x] Slint GUI with modern Qt-like styling
- [x] Profile save/load (TOML format in `~/.config/razerlinux/profiles/`)
- [x] Button remapping (evdev grab + uinput virtual device)
- [x] Modifier combos (Ctrl/Alt/Shift/Meta)
- [x] Side panel Driver Mode switching (automatic)
- [x] Windows-style middle-click autoscroll with visual overlay
- [x] Background DPI polling for real-time updates

### In Progress 🔄
- [ ] **Macro system** - 📝 Macros tab, record/build macros, assign to buttons
- [ ] Remap UX presets (numbers/F-keys/arrows), target capture button
- [ ] Per-panel defaults (2/7/12 buttons)
- [ ] Auto-detect side panel / button count from evdev

### Planned 📋
- [ ] **Import profiles from Windows Razer Synapse** (research phase)
- [ ] RGB lighting control
- [ ] Polling rate configuration
- [ ] More device support (other Razer mice)
- [ ] System tray integration
- [ ] Wayland overlay support (currently X11 only)
- [ ] RPM/DEB/AppImage packages
- [ ] Per-application profile switching

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

### Adding Support for New Devices

1. Find your device's USB VID:PID (e.g., `lsusb`)
2. Add the device ID to `device.rs`
3. Test the protocol commands
4. Submit a PR with your findings

## Troubleshooting

### "Permission denied" when accessing device

Make sure you've installed the udev rules and replugged the mouse:
```bash
ls -la /dev/hidraw*  # Should show crw-rw-rw- permissions
```

Ensure your user is in the `input` group:
```bash
groups  # Should include 'input'
sudo usermod -aG input $USER  # Add yourself if not
# Log out and log back in for changes to take effect
```

### Device not found

Check that your mouse is detected:
```bash
lsusb | grep 1532  # 1532 is Razer's USB Vendor ID
```

### Side buttons not detected during "Learn"

**UPDATE**: RazerLinux now handles side buttons automatically using Device Mode switching!

#### How It Works

The Razer Naga Trinity has two device modes:
- **Normal Mode (0x00)**: Side buttons send NO input (default)
- **Driver Mode (0x03)**: Side buttons send keyboard keys (1-9, 0, -, =)

When you enable button remapping in RazerLinux, the app automatically switches to Driver Mode. This makes all 12 side panel buttons send standard keyboard events that can be captured and remapped.

#### Side Button Key Mappings (in Driver Mode)

| Side Button | Key Sent |
|-------------|----------|
| 1 | KEY_1 |
| 2 | KEY_2 |
| 3 | KEY_3 |
| 4 | KEY_4 |
| 5 | KEY_5 |
| 6 | KEY_6 |
| 7 | KEY_7 |
| 8 | KEY_8 |
| 9 | KEY_9 |
| 10 | KEY_0 |
| 11 | KEY_MINUS |
| 12 | KEY_EQUAL |

**No OpenRazer kernel driver required!**

### Autoscroll overlay doesn't appear (X11)

The autoscroll visual indicator requires X11. If you're running Wayland:

```bash
# Check if you're on Wayland
echo $XDG_SESSION_TYPE
```

Current workarounds for Wayland users:
- Run RazerLinux under XWayland
- The autoscroll *functionality* still works, only the visual overlay is X11-only
- Wayland overlay support is planned for a future release

### Autoscroll feels too fast or too slow

You can adjust the constants in [src/remap.rs](src/remap.rs):

```rust
const SCROLL_THRESHOLD: i32 = 20;       // Pixels before scrolling starts
const SCROLL_SPEED_DIVISOR: i32 = 50;   // Higher = slower scrolling
const SCROLL_TICK_INTERVAL: u32 = 5;    // Scroll every N mouse events
```

After editing, rebuild with `cargo build`.

### GUI doesn't start

Ensure you have the required display libraries:
```bash
# For X11
sudo zypper install libX11-devel

# For Wayland
sudo zypper install wayland-devel
```

## License

GNU GPL v3.0 - see [LICENSE](LICENSE) for details.

Copyright (c) 2026 Aleksandar Milacic

## Acknowledgments

- [OpenRazer](https://github.com/openrazer/openrazer) - Protocol documentation and inspiration
- [Slint](https://slint.dev/) - Beautiful Rust GUI framework
- [hidapi](https://github.com/libusb/hidapi) - Cross-platform HID library

## Disclaimer

This project is not affiliated with Razer Inc. Use at your own risk.
