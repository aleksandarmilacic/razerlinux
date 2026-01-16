# Razer Mouse Mapping Solution for Linux

## Project Overview

A complete end-to-end solution for configuring and mapping Razer mice on Linux systems. This application will provide a graphical interface to:
- Detect and configure Razer mice
- Map mouse buttons to custom actions
- Configure DPI settings
- Set up lighting/RGB effects
- Create and manage profiles
- Persist settings across reboots

### Current Status (Jan 2026)

#### ✅ Completed Features

**Core Functionality:**
- ✅ Userspace HID working for Naga Trinity (DPI read/write, firmware, device mode switching)
- ✅ Slint GUI with "cosmic" dark theme
- ✅ TOML profiles stored under `~/.config/razerlinux/profiles/`
- ✅ Software remapping via evdev/uinput (key + Ctrl/Alt/Shift/Meta combos)
- ✅ Driver Mode switching (side buttons send KEY_1-KEY_12 automatically)
- ✅ Background DPI polling with real-time UI updates

**Macro System:**
- ✅ Full macro recording with key press/release and timing capture
- ✅ Persistent KeyCaptureListener for reliable keyboard input capture via evdev
- ✅ Macro storage with names, repeat counts, and action lists
- ✅ Macro playback testing via uinput virtual device
- ✅ Editable action list with selectable items and delete functionality
- ✅ Visual recording indicator in UI

**Settings & Startup:**
- ✅ **Autostart on system boot** - Creates XDG autostart desktop entry
- ✅ **Default profile loading** - Automatically loads selected profile on startup
- ✅ Settings persistence to `~/.config/razerlinux/settings.toml`
- ✅ Settings tab with startup options

**Autoscroll:**
- ✅ **Windows-Style Middle-Click Autoscroll** with visual overlay:
  - X11 overlay window with directional arrows
  - XShape extension for click-through (doesn't interfere with input)
  - Distance-based scroll speed (further from anchor = faster)
  - Throttled overlay updates to prevent X11 flooding
  - Free cursor movement during autoscroll

**Testing:**
- ✅ **Automated test suite** with 33+ tests covering:
  - Macro engine (recording, playback, actions management)
  - Profile serialization/deserialization
  - HID protocol report generation
  - Settings management
  - Integration tests for data formats
  - Hardware tests (ignored by default, run with `--ignored`)

#### 📁 Project Structure

```
~/.config/razerlinux/
├── settings.toml          # App settings (autostart, default profile)
└── profiles/
    └── *.toml             # Saved profiles
```

#### Technical Implementation Details

**Autoscroll Architecture:**
```
┌─────────────────────────────────────────────────────────────┐
│                     remap.rs (Remapper)                      │
│  - Captures middle-click to enter autoscroll mode            │
│  - Tracks cursor position relative to anchor point           │
│  - Emits scroll events every 5 mouse events                  │
│  - Scroll speed = distance / 50 (with 20px threshold)        │
│  - Sends UpdateDirection to overlay every 20 events          │
└──────────────────────────┬──────────────────────────────────┘
                           │ mpsc channel (OverlayCommand)
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                   overlay.rs (X11 Overlay)                   │
│  - 48x48px override_redirect window                          │
│  - XShape extension sets empty input region (click-through)  │
│  - Draws directional arrows based on scroll direction        │
│  - Throttled redraws (only if direction changed >0.2)        │
│  - Commands: Show, Hide, UpdateDirection(dx,dy), Shutdown    │
└─────────────────────────────────────────────────────────────┘
```

**Key Constants (remap.rs):**
- `SCROLL_THRESHOLD = 20` - Pixels before scrolling starts
- `SCROLL_SPEED_DIVISOR = 50` - Higher = slower scroll
- `SCROLL_TICK_INTERVAL = 5` - Emit scroll every N mouse events
- `DIRECTION_UPDATE_INTERVAL = 20` - Update overlay every N events

#### 🚧 Next Up
- Assign macros to mouse buttons (UI to bind macro → button)
- Remap UX presets (numbers/F-keys/arrows) and per-panel defaults
- Auto-detect side panel / button count from evdev capabilities
- RGB lighting control
- Wayland overlay support (currently X11 only)
- System tray with minimize-to-tray option
- Packaging (RPM/DEB/AppImage)

---

## Running Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run hardware tests (requires device + sudo)
sudo cargo test -- --ignored
```

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
- [x] DPI adjustment (100-16,000 in steps)
- [ ] Polling rate (125/500/1000 Hz)
- [x] Software button remapping (key + modifier combos via evdev/uinput)
- [x] Driver Mode switching (side buttons send keyboard keys)
- [x] Windows-style middle-click autoscroll with visual overlay
- [ ] Side panel auto-detection and per-panel defaults (2/7/12)
- [ ] RGB scroll wheel + logo lighting
- [ ] On-board profile storage (if supported)

---

## Architecture

### System Components (Userspace Approach - No Kernel Driver!)

```
┌─────────────────────────────────────────────────────────────────────┐
│                        GUI Application (Slint)                       │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌─────────────┐ │
│  │ Device Panel │ │ DPI Panel    │ │ Remap Panel  │ │ Profile Mgr │ │
│  └──────────────┘ └──────────────┘ └──────────────┘ └─────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        ▼                           ▼                           ▼
┌───────────────┐         ┌─────────────────────┐       ┌─────────────┐
│  device.rs    │         │     remap.rs        │       │ profile.rs  │
│  - DPI R/W    │         │  - evdev grab       │       │ - TOML save │
│  - Mode switch│         │  - uinput emit      │       │ - TOML load │
│  - Firmware   │         │  - Autoscroll logic │       │ - Mappings  │
└───────┬───────┘         └──────────┬──────────┘       └─────────────┘
        │                            │
        ▼                            │ mpsc channel
┌───────────────┐                    ▼
│   hidapi      │         ┌─────────────────────┐
│   (USB HID)   │         │    overlay.rs       │
└───────┬───────┘         │  - X11 window       │
        │                 │  - XShape click-thru│
        ▼                 │  - Direction arrows │
┌───────────────┐         └─────────────────────┘
│  USB Device   │
│  Naga Trinity │
└───────────────┘
```

**Source Files:**
| File | Purpose |
|------|---------|
| `main.rs` | GUI setup, callbacks, lifecycle management |
| `device.rs` | USB HID communication (DPI, mode switching, firmware) |
| `protocol.rs` | Razer 90-byte report structure, CRC calculation |
| `profile.rs` | TOML profile save/load in `~/.config/razerlinux/profiles/` |
| `remap.rs` | evdev capture, uinput virtual device, autoscroll state machine |
| `overlay.rs` | X11 autoscroll overlay with XShape for click-through |
| `hidpoll.rs` | Background HID polling for real-time DPI updates |

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

### Phase 1: Foundation (MVP) - Naga Trinity Focus ✅ COMPLETE
- [x] Set up project structure
- [x] Implement USB HID device detection (hidapi)
- [x] Send/receive basic commands to Naga Trinity
- [x] Basic GUI with device info display (Slint)
- [x] Read current DPI settings
- [x] Set DPI via GUI
- [x] Background DPI polling for real-time updates

### Phase 2: Button Mapping ✅ COMPLETE
- [x] Capture button events via evdev
- [x] Create virtual input device (uinput)
- [x] Map side buttons to keyboard keys
- [x] Map buttons with modifier combos (Ctrl/Alt/Shift/Meta)
- [x] Driver Mode switching (side buttons send KEY_1-12)
- [x] Save/load button mappings
- [x] **Windows-style middle-click autoscroll with visual overlay**
- [ ] Side panel auto-detection (2/7/12 button modes) - PENDING
- [ ] Button learning UX improvements (target capture) - PENDING

### Phase 3: Profiles ✅ COMPLETE
- [x] Profile data structure
- [x] Profile storage (TOML format)
- [x] Profile switching UI
- [x] Default profile on startup
- [ ] Systemd user service for persistence - PENDING

### Phase 4: Advanced Features 🔄 IN PROGRESS
- [ ] Macro recording/playback
- [ ] RGB scroll wheel + logo control
- [ ] Polling rate configuration
- [ ] System tray daemon
- [ ] Per-application profiles (optional)
- [ ] Wayland overlay support (currently X11 only)

#### Macro System Design (Phase 4 Priority)

**Macro Data Structure:**
```rust
struct MacroAction {
    action_type: MacroActionType,  // KeyPress, KeyRelease, Delay, MouseClick
    key_code: Option<u16>,         // For key actions
    delay_ms: Option<u32>,         // For delay actions
}

enum MacroActionType {
    KeyPress,      // Press a key
    KeyRelease,    // Release a key
    Delay,         // Wait N milliseconds
    MouseClick,    // Click mouse button
}

struct Macro {
    id: u32,
    name: String,
    actions: Vec<MacroAction>,
    repeat_count: u32,        // 0 = infinite while held
    repeat_delay_ms: u32,     // Delay between repeats
}
```

**Macro Builder UI (📝 Macros Tab):**
- List of saved macros with Add/Edit/Delete
- Recording mode: capture keystrokes in real-time
- Manual mode: add actions one by one
- Delay insertion between actions
- Preview/test macro before saving
- Assign macro to button (link to Button Mapping tab)

**Macro Assignment:**
- In Button Mapping tab, target dropdown includes "Macro: [name]"
- Macros stored in profile alongside button mappings

### Phase 5: Polish & Distribution 📋 PLANNED
- [ ] RPM packaging for openSUSE
- [ ] DEB packaging for Debian/Ubuntu
- [ ] AppImage for universal Linux
- [ ] Auto-start configuration
- [ ] User documentation (README updated ✅)
- [ ] Support additional Razer mice
- [ ] **Import profiles from Windows Razer Synapse** (research needed)

#### Razer Synapse Profile Import (Research Required)

**Synapse Profile Locations (Windows):**
- Synapse 2: `%AppData%\Razer\Synapse\Profiles\`
- Synapse 3: `%LocalAppData%\Razer\Synapse3\Profiles\` or cloud-synced

**Potential Approaches:**
1. Parse Synapse XML/JSON profile files
2. Export from Synapse → convert script
3. Manual profile recreation wizard

**Research TODO:**
- [ ] Document Synapse profile file format
- [ ] Identify which settings can be imported (DPI, macros, lighting?)
- [ ] Create profile conversion utility

---

### Current Progress Summary (Jan 2026)

| Phase | Status | Completion |
|-------|--------|------------|
| Phase 1: Foundation | ✅ Complete | 100% |
| Phase 2: Button Mapping | ✅ Complete | ~90% (auto-detect pending) |
| Phase 3: Profiles | ✅ Complete | ~85% (systemd pending) |
| Phase 4: Advanced Features | 🔄 In Progress | ~20% (macros next) |
| Phase 5: Polish & Distribution | 📋 Planned | 0% (Synapse import planned) |

**Next Priority: Macro System**
- Add 📝 Macros tab to UI
- Implement macro data structures
- Macro recording/playback
- Assign macros to buttons

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

## Troubleshooting: Side Button Detection

### Problem

Side buttons on the Razer Naga Trinity (especially the 12-button grid panel) are **NOT detected at all** during the "Learn" button capture process. Only left, right, and middle mouse buttons work.

### Root Cause Analysis (CONFIRMED AND SOLVED)

After extensive investigation including raw HID analysis, evdev monitoring, and OpenRazer source code review, the root cause has been identified and a solution implemented:

**The Naga Trinity has two device modes:**
- **Normal Mode (0x00)**: Side buttons send NO input at all
- **Driver Mode (0x03)**: Side buttons send keyboard keys (1-9, 0, -, =)

The device defaults to Normal Mode, which is why side buttons weren't detected. Switching to Driver Mode enables side button functionality!

### Solution Implemented

RazerLinux now automatically manages device mode:

1. **On remapping enable**: Switches device to Driver Mode (0x03)
2. **On remapping disable**: Restores Normal Mode (0x00)
3. **On app startup**: Ensures device is in Normal Mode (clean state)

In Driver Mode, the side buttons send standard keyboard key events:
| Side Button | Key Code | Key Name |
|-------------|----------|----------|
| 1 | KEY_1 (2) | 1 |
| 2 | KEY_2 (3) | 2 |
| 3 | KEY_3 (4) | 3 |
| 4 | KEY_4 (5) | 4 |
| 5 | KEY_5 (6) | 5 |
| 6 | KEY_6 (7) | 6 |
| 7 | KEY_7 (8) | 7 |
| 8 | KEY_8 (9) | 8 |
| 9 | KEY_9 (10) | 9 |
| 10 | KEY_0 (11) | 0 |
| 11 | KEY_MINUS (12) | - |
| 12 | KEY_EQUAL (13) | = |

### Technical Details

1. **Device Mode is correct**: The device reports mode 0x00 (Normal mode) which should send keyboard keypresses from side buttons.

2. **Interfaces detected correctly**:
   | Device | Name | Interface | Purpose |
   |--------|------|-----------|---------|
   | hidraw3 | Naga Trinity | input0 | Mouse + vendor data |
   | hidraw4 | Naga Trinity | input1 | Keyboard interface |
   | hidraw5 | Naga Trinity | input2 | Keyboard interface |
   | event8 | Naga Trinity | input0 | 5 mouse buttons only |
   | event9/11 | Naga Trinity Keyboard | input1/2 | Keyboard keys |

3. **evdev receives NO events from side buttons**: Tested with `evtest` on all interfaces - absolutely no events when side buttons are pressed.

4. **hidraw3 receives raw data**: Vendor-specific HID reports (Usage Page 0xFF00) are received on the mouse interface but NOT translated to evdev events.

5. **OpenRazer kernel driver required**: The OpenRazer project has a kernel driver (`razermouse`) with a special `razer_raw_event()` function that intercepts and parses these vendor-specific reports. Without this driver, the side buttons simply don't work on Linux.

### HID Report Structure (Mouse Interface)

The Naga Trinity mouse interface (hidraw3) sends:
- 5 button bits (standard mouse buttons)
- 3 padding bits
- **2 bytes of vendor-specific data** (Usage Page 0xFF00, Usage 0x40) ← Side panel data here!
- 1 byte wheel
- 2 x 16-bit X/Y position

The vendor-specific bytes contain side panel button states, but `hid-generic` ignores them.

### Solutions

#### Solution 1: RazerLinux Driver Mode (Implemented ✓)

RazerLinux now handles this automatically! When you enable button remapping:
1. The app sends a USB control transfer to switch the device to Driver Mode
2. Side buttons start sending keyboard key events
3. The remapper captures and remaps these events
4. When you disable remapping, Normal Mode is restored

No kernel drivers or additional software needed!

#### Alternative: Install OpenRazer

```bash
# openSUSE
sudo zypper addrepo https://download.opensuse.org/repositories/hardware/openSUSE_Leap_15.6/hardware.repo
sudo zypper refresh
sudo zypper install openrazer-driver openrazer-daemon

# Enable and start
sudo modprobe razermouse
systemctl --user enable openrazerdaemon
systemctl --user start openrazerdaemon
```

Once OpenRazer is installed, side buttons will appear as standard keyboard events (KEY_1 through KEY_12 or F13-F24 depending on profile).

#### Option 2: Use input-remapper

The [input-remapper](https://github.com/sezanzeb/input-remapper) project handles Razer mice with side panels and can remap buttons without requiring kernel drivers for some use cases.

```bash
sudo zypper install input-remapper
```

#### Option 3: Userspace HID parsing (Future Enhancement)

We could implement parsing of the vendor-specific HID reports directly from `/dev/hidraw3`. This would require:
1. Opening hidraw3 in non-blocking mode
2. Parsing the incoming HID reports (8-byte mouse reports)
3. Extracting side button state from bytes 6-7 (vendor-specific data)
4. Injecting synthetic evdev events via uinput

This is complex but would provide a fully userspace solution without kernel drivers.

### How to Check if Side Panel is Working

```bash
# 1. Check device mode (should be 0x00)
# Run razerlinux and check logs for "Device mode: 0x00"

# 2. Monitor all evdev interfaces
sudo evtest /dev/input/event8  # Mouse buttons
sudo evtest /dev/input/event9  # Should see side buttons IF working

# 3. Monitor raw HID (requires sudo)
sudo xxd /dev/hidraw3 | head -50
# Press side buttons - look for changing patterns in bytes 6-7
```

### Physical Side Panel Check

The Naga Trinity has **interchangeable side panels**:
- 2-button panel (FPS)
- 7-button ring (MOBA)
- 12-button grid (MMO)

Ensure a side panel is properly attached and seated. The magnetic connection should click firmly.

---

## Understanding the Naga Trinity Architecture

### Why Side Buttons Don't Work Without Drivers

The Razer Naga Trinity is a "composite" USB device with 3 HID interfaces:

```
USB Device 1532:0067 (Naga Trinity)
├── Interface 0: Mouse (5 buttons + vendor extension)
├── Interface 1: Keyboard (for side panel buttons)
└── Interface 2: System Control / Consumer (media keys)
```

The side panel buttons are designed to send **standard keyboard keycodes** (like "1", "2", etc.) through Interface 1. However, the way these are encoded in the HID reports requires special handling:

1. **Report ID 0x04**: Side button events use a special 16-byte report format
2. **Non-standard encoding**: Button states are encoded differently than standard keyboards
3. **OpenRazer translates these**: The kernel driver converts Report ID 4 data into proper evdev KEY events

Without OpenRazer, the HID reports are received but not understood by `hid-generic`.

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
