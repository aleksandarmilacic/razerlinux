# 🐍 RazerLinux

A userspace Razer mouse configuration tool for Linux. No kernel drivers required!

![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)
![License](https://img.shields.io/badge/license-GPLv3-blue)
![Platform](https://img.shields.io/badge/platform-Linux-green)

## Features

- **Direct USB HID Communication** - Talks directly to your Razer mouse via hidapi, no kernel driver needed
- **DPI Configuration** - Read and set DPI (100-16000) with independent X/Y axis control
- **Profile Management** - Save and load configuration profiles to TOML files
- **Button Remapping (Software)** - evdev grab + uinput virtual device with modifier combos (Ctrl/Alt/Shift/Meta)
- **Modern GUI** - Clean Qt-like interface built with Slint
- **Lightweight** - Pure Rust, minimal dependencies

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

[[remap.mappings]]
source = 1          # mouse button code (e.g., side button)
target = 2          # target key code (e.g., KEY_1)
ctrl = false
alt = false
shift = false
meta = false
```

## Architecture

```
razerlinux/
├── src/
│   ├── main.rs       # GUI application entry point
│   ├── device.rs     # USB HID device communication
│   ├── protocol.rs   # Razer USB protocol implementation
│   ├── profile.rs    # Profile save/load management
│   └── remap.rs      # evdev/uinput software remapper
├── ui/
│   └── main.slint    # Slint GUI definition
└── config/
    └── 99-razermouse.rules  # udev rules for permissions
```

### USB Protocol

RazerLinux implements the Razer USB HID protocol:
- 90-byte feature reports
- CRC verification (XOR of bytes 2-87)
- Transaction ID 0xFF for older devices (Naga Trinity)
- Support for DPI, polling rate, and device queries

## Roadmap

- [x] USB HID protocol implementation
- [x] DPI read/write
- [x] Slint GUI
- [x] Profile save/load
- [x] Button remapping (evdev/uinput) — basic key + modifier combos
- [ ] Remap UX presets (numbers/F-keys/arrows), target capture, per-panel defaults (2/7/12 buttons)
- [ ] Auto-detect side panel / button count from evdev and prefill mappings
- [ ] RGB lighting control
- [ ] More device support
- [ ] System tray integration
- [ ] RPM/DEB/AppImage packages

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

#### Alternative: Install OpenRazer (Optional)

```bash
# openSUSE
sudo zypper addrepo https://download.opensuse.org/repositories/hardware/openSUSE_Leap_15.6/hardware.repo
sudo zypper refresh
sudo zypper install openrazer-driver openrazer-daemon
sudo modprobe razermouse
systemctl --user enable --now openrazerdaemon

# Fedora
sudo dnf install kernel-devel
sudo dnf copr enable morbidick/openrazer
sudo dnf install openrazer-driver openrazer-daemon

# Ubuntu/Debian
sudo add-apt-repository ppa:openrazer/stable
sudo apt update
sudo apt install openrazer-driver-dkms openrazer-daemon
```

After installing OpenRazer, reboot or reload the kernel module, then replug your mouse. Side buttons should now send keyboard events (KEY_1 through KEY_12 or similar).

#### Verifying Side Button Detection

```bash
# Check if side buttons generate events (requires evtest)
sudo zypper install evtest  # or apt/dnf equivalent
sudo evtest /dev/input/event9  # Try different event numbers
# Press side buttons - if working, you'll see KEY_* events
```

#### Without OpenRazer

If you cannot install OpenRazer, consider using [input-remapper](https://github.com/sezanzeb/input-remapper) which may provide alternative handling for some Razer devices.

**Note**: RazerLinux's DPI, polling rate, and profile features work WITHOUT OpenRazer. Only side panel button detection requires the kernel driver.

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
