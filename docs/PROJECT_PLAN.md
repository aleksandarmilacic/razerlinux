# Razer Mouse Mapping Solution for Linux

## Project Overview

A complete end-to-end solution for configuring and mapping Razer mice on Linux systems. This application will provide a graphical interface to:
- Detect and configure Razer mice
- Map mouse buttons to custom actions
- Configure DPI settings
- Set up lighting/RGB effects
- Create and manage profiles
- Persist settings across reboots

---

## Problem Statement

Linux lacks official Razer Synapse support, leaving users without:
- Button remapping capabilities
- DPI adjustment tools
- RGB/lighting control
- Profile management
- Per-application configurations

---

## Requirements

### Functional Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-01 | Detect connected Razer mice | Must Have |
| FR-02 | Display device information (model, firmware, serial) | Must Have |
| FR-03 | Remap all mouse buttons | Must Have |
| FR-04 | Assign keyboard shortcuts to buttons | Must Have |
| FR-05 | Assign macros/sequences to buttons | Should Have |
| FR-06 | Configure DPI levels (up to 5 stages) | Must Have |
| FR-07 | Set polling rate | Should Have |
| FR-08 | Control RGB lighting effects | Should Have |
| FR-09 | Create/save/load profiles | Must Have |
| FR-10 | Auto-switch profiles per application | Could Have |
| FR-11 | System tray integration | Should Have |
| FR-12 | Import/export configurations | Should Have |
| FR-13 | Support multiple mice simultaneously | Could Have |

### Non-Functional Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| NFR-01 | Start on system boot | Must Have |
| NFR-02 | Low memory footprint (<50MB) | Should Have |
| NFR-03 | Minimal CPU usage when idle | Must Have |
| NFR-04 | Settings persist across reboots | Must Have |
| NFR-05 | Work without root (after initial setup) | Should Have |
| NFR-06 | Support major distros (openSUSE, Ubuntu, Fedora, Arch) | Must Have |
| NFR-07 | Wayland and X11 support | Must Have |

---

## Development Environment

**Primary Platform:** openSUSE Linux

### openSUSE-Specific Notes

**Package Manager:** zypper

**OpenRazer Installation:**
```bash
# Add hardware repo (openSUSE Tumbleweed)
sudo zypper addrepo https://download.opensuse.org/repositories/hardware/openSUSE_Tumbleweed/hardware.repo
sudo zypper refresh
sudo zypper install openrazer-meta

# Add user to required groups
sudo gpasswd -a $USER plugdev
```

**Development Dependencies:**
```bash
# Python + Qt development
sudo zypper install python3-devel python3-qt6 python3-evdev python3-pip

# USB HID userspace library (KEY DEPENDENCY!)
sudo zypper install hidapi hidapi-devel python3-hidapi

# Build essentials
sudo zypper install gcc gcc-c++ make cmake

# For Rust development (alternative)
sudo zypper install rust cargo gtk4-devel
```

**Udev Rule for Device Access (no root needed after this!):**
```bash
# /etc/udev/rules.d/99-razermouse.rules
SUBSYSTEM=="usb", ATTR{idVendor}=="1532", ATTR{idProduct}=="0067", MODE="0666"
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1532", ATTRS{idProduct}=="0067", MODE="0666"
```

```bash
# Install udev rule
sudo cp config/99-razermouse.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
# Replug mouse - now works without root!
```

**Key Differences from Other Distros:**
- Uses `zypper` instead of apt/dnf/pacman
- Package names may differ (e.g., `python3-qt6` vs `python3-pyqt6`)
- OpenRazer available via OBS hardware repository
- SUSE uses `plugdev` group for device access

---

## Target Device

### Razer Naga Trinity

| Property | Value |
|----------|-------|
| USB Vendor ID | `1532` (Razer) |
| USB Product ID | `0067` |
| Max DPI | 16,000 |
| Polling Rate | Up to 1000Hz |
| RGB | Yes (Chroma) |
| Side Panels | 3 interchangeable |

**Button Configurations by Panel:**
- **2-Button Panel:** 2 side buttons (total ~7 buttons)
- **7-Button Ring:** Circular 7-button arrangement (total ~12 buttons)  
- **12-Button Grid:** MMO grid layout (total ~19 buttons)

**Features to Support:**
- [ ] DPI adjustment (100-16,000 in steps)
- [ ] Polling rate (125/500/1000 Hz)
- [ ] All button remapping
- [ ] Side panel auto-detection
- [ ] RGB scroll wheel + logo lighting
- [ ] On-board profile storage (if supported)

---

## Architecture

### System Components (Userspace Approach - No Kernel Driver!)

```
┌─────────────────────────────────────────────────────────────┐
│                      GUI Application                         │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐ │
│  │ Device Panel │ │ Button Panel │ │ Lighting/DPI Panel   │ │
│  └──────────────┘ └──────────────┘ └──────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Core Service/Daemon                       │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐ │
│  │ Device Mgr   │ │ Profile Mgr  │ │ Input Remapper       │ │
│  └──────────────┘ └──────────────┘ └──────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              │
            ┌─────────────────┼─────────────────┐
            ▼                 ▼                 ▼
     ┌─────────────┐  ┌─────────────────┐  ┌─────────────┐
     │ hidapi      │  │ uinput/evdev    │  │ Config      │
     │ (USB HID)   │  │ (Input Layer)   │  │ Storage     │
     └─────────────┘  └─────────────────┘  └─────────────┘
            │
            ▼
     ┌─────────────┐
     │ USB Device  │
     │ Naga Trinity│
     └─────────────┘
```

### Why Userspace (No OpenRazer)?

| Aspect | OpenRazer (Kernel) | Our Approach (Userspace) |
|--------|-------------------|--------------------------|
| Installation | DKMS + kernel headers + compile | Single package + udev rule |
| Dependencies | Heavy | Minimal (hidapi only) |
| Permissions | plugdev group + module load | udev rule only |
| Updates | Rebuild on kernel update | No rebuild needed |
| Portability | Linux only | Could port to other OS |
| Complexity | External dependency | Self-contained |

### Component Responsibilities

#### 1. GUI Application
- User interface for all configuration
- Real-time preview of settings
- Profile management UI
- System tray icon

#### 2. Core Service/Daemon
- Runs in background (no root needed after setup!)
- Handles USB HID communication via hidapi
- Manages button remapping via virtual input
- Applies settings on device connect

#### 3. Device Layer
- **hidapi**: Userspace USB HID communication (no kernel driver!)
- **uinput/evdev**: Linux input subsystem for button remapping
- **libudev**: Device hotplug detection

### USB HID Protocol

We'll implement the Razer USB protocol directly. The protocol is documented through OpenRazer's reverse engineering:

**Report Structure:**
```
Byte 0:    Status (0x00 = new command)
Byte 1:    Transaction ID
Byte 2:    Remaining packets (0x00 for single)
Byte 3:    Protocol type (0x00)
Byte 4:    Data size
Byte 5:    Command class
Byte 6:    Command ID
Byte 7-86: Arguments (80 bytes)
Byte 87:   CRC
Byte 88:   Reserved (0x00)
```

**Key Commands for Naga Trinity:**
| Command | Class | ID | Description |
|---------|-------|-----|-------------|
| Set DPI | 0x04 | 0x05 | Set DPI (X and Y) |
| Get DPI | 0x04 | 0x85 | Read current DPI |
| Set Poll Rate | 0x00 | 0x05 | Set polling rate |
| Set LED | 0x03 | 0x00 | Control RGB lighting |
| Get Firmware | 0x00 | 0x81 | Read firmware version |

---

## Technology Stack (DECIDED)

### ✅ Rust + Qt (via qml-rs or slint)

**Language:** Rust 🦀
**GUI Framework:** Qt/QML (via `cxx-qt` or `slint` as Qt-like alternative)
**Config Format:** TOML

| Component | Library |
|-----------|---------|
| GUI | `cxx-qt` (Qt bindings) or `slint` (Qt-like, pure Rust) |
| USB HID | `hidapi` crate |
| Input Events | `evdev` crate |
| Virtual Input | `uinput` crate |
| Device Detection | `udev` crate |
| Async Runtime | `tokio` |
| Config | `serde` + `toml` |
| Logging | `tracing` |

### Why This Stack?

- **Single binary** - No runtime dependencies
- **Native Qt look** - Perfect on KDE Plasma / openSUSE
- **Memory safe** - No crashes from memory bugs
- **Fast** - Native performance, low resource usage
- **Easy distribution** - RPM or just copy the binary

### Rust Qt Options

**Option A: cxx-qt**
- Direct Qt bindings for Rust
- Use QML for UI, Rust for logic
- Most "real Qt" experience

**Option B: Slint (Recommended for simplicity)**
- Qt-like but pure Rust, no C++ needed
- Looks native on all platforms
- Easier to learn and build
- Compiles to native code
- Great documentation

**Recommendation:** Start with **Slint** - it's easier to set up and still looks professional. We can switch to full Qt later if needed.

---

### openSUSE Development Setup

```bash
# Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# System dependencies
sudo zypper install hidapi-devel libudev-devel gcc

# For Slint GUI
# (no extra deps needed - pure Rust!)

# For cxx-qt (full Qt)
sudo zypper install qt6-base-devel qt6-declarative-devel cmake
```

---

## Technology Options (Considered)

### Option 1: Python + Qt (PyQt6/PySide6)

**Pros:**
- Rapid development
- `hidapi` Python bindings available
- Cross-platform GUI with native look
- Large ecosystem (evdev, python-uinput)
- Easy to contribute to

**Cons:**
- Larger memory footprint
- Requires Python runtime
- Packaging can be complex (but PyInstaller works)

**Key Libraries:**
- `hidapi` - USB HID communication (userspace!)
- `python-evdev` - Input event handling
- `python-uinput` - Virtual input device
- `PyQt6` or `PySide6` - GUI framework
- `pyudev` - Device hotplug detection

---

### Option 2: Rust + GTK4/Iced

**Pros:**
- Native performance
- Small binary, no runtime needed
- Memory safe
- Growing Linux desktop ecosystem
- Single binary distribution
- `hidapi` crate available

**Cons:**
- Steeper learning curve
- Less mature GUI libraries

**Key Libraries:**
- `gtk4-rs` or `iced` - GUI framework
- `hidapi` - USB HID communication
- `evdev` - Input handling
- `tokio` - Async runtime
- `udev` - Device detection

---

### Option 3: C++ + Qt6

**Pros:**
- Mature and battle-tested
- Native Qt integration
- Excellent performance
- Good packaging support

**Cons:**
- Manual memory management
- Longer development time
- Steeper learning curve

**Key Libraries:**
- `Qt6` - GUI and system integration
- `libevdev` - Input event handling
- `libudev` - Device enumeration
- `libusb` - USB communication

---

### Option 4: Go + Fyne/GTK

**Pros:**
- Fast compilation
- Single binary distribution
- Good concurrency model
- Growing ecosystem

**Cons:**
- GUI libraries less mature
- Larger binary size
- CGO dependency for some libraries

---

### Option 5: Electron/Tauri + Web Tech

**Pros:**
- Modern UI possibilities
- Rapid prototyping
- Tauri provides small binaries

**Cons:**
- Electron is resource-heavy
- Tauri still maturing
- Web tech overhead

---

## Recommendation Matrix

| Criteria | Python+Qt | Rust+GTK | C+++Qt | Go+Fyne |
|----------|-----------|----------|--------|---------|
| Dev Speed | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |
| Performance | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| Memory | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| OpenRazer Support | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐ |
| Packaging | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| Maintainability | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |

---

## Existing Projects (Research)

### OpenRazer
- URL: https://openrazer.github.io/
- Kernel driver approach (DKMS)
- **We use their protocol research, not the driver**
- Great reference for USB HID commands

### razercfg
- URL: https://bues.ch/cms/hacking/razercfg.html
- Older userspace tool
- Some mice supported via libusb
- Good protocol reference

### Input Remapper
- URL: https://github.com/sezanzeb/input-remapper
- Generic input remapping tool
- Good reference for evdev/uinput usage

### Polychromatic
- URL: https://polychromatic.app/
- Depends on OpenRazer
- Lighting focused, limited remapping

---

## Implementation Phases

### Phase 1: Foundation (MVP) - Naga Trinity Focus
- [ ] Set up project structure
- [ ] Implement USB HID device detection (hidapi)
- [ ] Send/receive basic commands to Naga Trinity
- [ ] Basic GUI with device info display
- [ ] Read current DPI settings
- [ ] Set DPI via GUI

### Phase 2: Button Mapping
- [ ] Capture button events via evdev
- [ ] Create virtual input device (uinput)
- [ ] Map side buttons to keyboard keys
- [ ] Map buttons to other mouse buttons
- [ ] Side panel detection (2/7/12 button modes)
- [ ] Save/load button mappings

### Phase 3: Profiles
- [ ] Profile data structure
- [ ] Profile storage (JSON)
- [ ] Profile switching UI
- [ ] Default profile on startup
- [ ] Systemd user service for persistence

### Phase 4: Advanced Features
- [ ] Macro recording/playback
- [ ] RGB scroll wheel + logo control
- [ ] Polling rate configuration
- [ ] System tray daemon
- [ ] Per-application profiles (optional)

### Phase 5: Polish & Distribution
- [ ] RPM packaging for openSUSE
- [ ] AppImage for universal Linux
- [ ] Auto-start configuration
- [ ] User documentation
- [ ] Support additional Razer mice

---

## Technical Considerations

### Button Remapping Approach

**Option A: Grab + Virtual Device**
```
Physical Mouse → Grab Events → Remap Logic → Virtual Device → System
```
- Grab physical device exclusively
- Process events and remap
- Emit remapped events via uinput

**Option B: Interception at evdev level**
- Use evdev to read events
- Block original events
- Inject remapped events

### Wayland Considerations
- Wayland has stricter input security
- May need compositor-specific integration
- libinput is the standard input library
- Consider using libei for input emulation

### Permissions
- uinput requires `input` group membership
- OpenRazer requires `plugdev` group
- Consider udev rules for automatic permissions

---

## File Structure (Proposed)

```
razerlinux/
├── docs/
│   ├── PROJECT_PLAN.md
│   └── USER_GUIDE.md
├── src/
│   ├── main.rs              # Application entry point
│   ├── lib.rs               # Library root
│   ├── device/
│   │   ├── mod.rs           # Device module
│   │   ├── hid.rs           # USB HID communication
│   │   ├── protocol.rs      # Razer USB protocol
│   │   └── naga_trinity.rs  # Naga Trinity specific
│   ├── input/
│   │   ├── mod.rs           # Input module
│   │   ├── remapper.rs      # Button remapping logic
│   │   └── virtual_device.rs # uinput virtual device
│   ├── profile/
│   │   ├── mod.rs           # Profile module
│   │   └── manager.rs       # Profile save/load/switch
│   ├── gui/
│   │   ├── mod.rs           # GUI module
│   │   ├── app.rs           # Main application window
│   │   └── components/      # UI components
│   └── config/
│       └── mod.rs           # Configuration handling
├── ui/                      # Slint UI files (.slint)
│   ├── main.slint
│   ├── device_panel.slint
│   ├── button_panel.slint
│   └── dpi_panel.slint
├── resources/
│   └── icons/
├── config/
│   ├── 99-razermouse.rules  # udev rules
│   └── default_profile.toml
├── Cargo.toml               # Rust dependencies
├── build.rs                 # Build script
└── README.md
```

---

## Questions for Decision

1. ✅ **Technology stack** - Rust + Qt/Slint
2. ⬜ **Slint vs full Qt (cxx-qt)?** - Recommend Slint for easier start
3. ⬜ **Config format** - TOML (Rust standard)
4. ⬜ **Packaging priority** - RPM for openSUSE + static binary

---

## Next Steps

1. ✅ Create project documentation (this document)
2. ✅ Decide on userspace approach (hidapi, no kernel driver)
3. ✅ Target device: Razer Naga Trinity
4. ✅ Decide on technology stack: **Rust + Qt/Slint**
5. ⬜ Set up Rust project structure
6. ⬜ Implement USB HID communication prototype
7. ⬜ Test reading DPI from Naga Trinity
8. ⬜ Build basic GUI shell
