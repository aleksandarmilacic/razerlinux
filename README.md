# 🐍 RazerLinux

A userspace Razer mouse configuration tool for Linux. No kernel drivers required!

![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)
![License](https://img.shields.io/badge/license-GPLv3-blue)
![Platform](https://img.shields.io/badge/platform-Linux-green)

## Features

- **Direct USB HID Communication** - Talks directly to your Razer mouse via hidapi, no kernel driver needed
- **DPI Configuration** - Read and set DPI (100-16000) with independent X/Y axis control
- **Profile Management** - Save and load configuration profiles to TOML files
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
git clone https://github.com/yourusername/razerlinux.git
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
```

## Architecture

```
razerlinux/
├── src/
│   ├── main.rs       # GUI application entry point
│   ├── device.rs     # USB HID device communication
│   ├── protocol.rs   # Razer USB protocol implementation
│   └── profile.rs    # Profile save/load management
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
- [ ] Button remapping (evdev/uinput)
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

### Device not found

Check that your mouse is detected:
```bash
lsusb | grep 1532  # 1532 is Razer's USB Vendor ID
```

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

Copyright (c) 2026 Aleksandar Milacic

## Acknowledgments

- [OpenRazer](https://github.com/openrazer/openrazer) - Protocol documentation and inspiration
- [Slint](https://slint.dev/) - Beautiful Rust GUI framework
- [hidapi](https://github.com/libusb/hidapi) - Cross-platform HID library

## Disclaimer

This project is not affiliated with Razer Inc. Use at your own risk.
