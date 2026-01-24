# Wayland Support Plan for RazerLinux

## Executive Summary

This document outlines a plan to add Wayland support to RazerLinux. Currently, the application relies on X11 for:
1. **Scrollable area detection** (`scroll_detect_x11.rs`)
2. **Autoscroll overlay indicator** (`overlay.rs`)

The core functionality (HID device communication, evdev input remapping, profile management) works on both X11 and Wayland since it operates at the kernel/udev level.

---

## Current X11 Dependencies

| Component | File | X11 API Used | Purpose |
|-----------|------|--------------|---------|
| Scroll Detection | `scroll_detect_x11.rs` | `QueryPointer`, `_NET_WM_WINDOW_TYPE`, `WM_CLASS`, XShape | Determine if cursor is over scrollable area |
| Overlay Indicator | `overlay.rs` | X11 window creation, XShape (input passthrough) | Show Windows-style autoscroll icon |
| Cursor Position | `remap.rs` | `QueryPointer` | Get cursor position for scroll direction |

---

## Wayland Architecture Challenges

Unlike X11, Wayland was designed with security and isolation in mind:

| Challenge | X11 Approach | Wayland Reality |
|-----------|--------------|-----------------|
| **Window inspection** | Any client can query any window | Clients are sandboxed, no cross-client access |
| **Cursor position** | Global `QueryPointer` | Only available to compositor or via protocols |
| **Overlay windows** | `override_redirect` + XShape | Requires compositor-specific layer-shell protocol |
| **Input passthrough** | XShape input region | Must use `zwlr_layer_shell_v1` or similar |
| **Window type detection** | `_NET_WM_WINDOW_TYPE` property | No equivalent - apps don't expose internals |

---

## Proposed Solution: Layered Abstraction

### Phase 1: Backend Trait Abstraction

Create a display-server-agnostic trait system:

```rust
// src/display_backend.rs

pub enum DisplayBackend {
    X11(X11Backend),
    Wayland(WaylandBackend),
    #[cfg(target_os = "macos")]
    MacOS(MacOSBackend),
}

pub trait ScrollDetector: Send + Sync {
    /// Check if cursor is over a scrollable area
    fn should_autoscroll(&self) -> bool;
    
    /// Get current cursor position (screen coordinates)
    fn cursor_position(&self) -> Option<(i32, i32)>;
    
    /// Clear any internal caches
    fn clear_cache(&self);
}

pub trait OverlayDisplay: Send {
    /// Show overlay at current cursor position
    fn show(&self) -> Result<()>;
    
    /// Hide overlay
    fn hide(&self) -> Result<()>;
    
    /// Update scroll direction indicator
    fn update_direction(&self, dx: f32, dy: f32);
    
    /// Shutdown the overlay system
    fn shutdown(self);
}
```

### Phase 2: Runtime Detection

```rust
// src/display_backend.rs

pub fn detect_backend() -> DisplayBackend {
    // Check XDG_SESSION_TYPE first (most reliable)
    if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
        match session_type.as_str() {
            "wayland" => return create_wayland_backend(),
            "x11" => return create_x11_backend(),
            _ => {}
        }
    }
    
    // Fallback: check WAYLAND_DISPLAY
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return create_wayland_backend();
    }
    
    // Fallback: check DISPLAY (X11)
    if std::env::var("DISPLAY").is_ok() {
        return create_x11_backend();
    }
    
    // Headless or unknown - use null backend
    DisplayBackend::Null
}
```

---

## Phase 3: Wayland Scroll Detection

### Option A: AT-SPI (Accessibility API) - RECOMMENDED

AT-SPI works on **both X11 and Wayland** since it's a D-Bus protocol:

```rust
// src/scroll_detect_wayland.rs (also works on X11!)

use atspi::{
    connection::AccessibilityConnection,
    registry::AccessibilityRegistry,
    Role,
};

pub struct ScrollDetectorAtSpi {
    connection: AccessibilityConnection,
    // Roles that indicate scrollable content
    scrollable_roles: HashSet<Role>,
    // Roles that indicate non-scrollable UI
    deny_roles: HashSet<Role>,
}

impl ScrollDetectorAtSpi {
    pub fn new() -> Result<Self> {
        let connection = AccessibilityConnection::open()?;
        
        let scrollable_roles = [
            Role::ScrollPane,
            Role::Viewport,
            Role::DocumentWeb,
            Role::DocumentText,
            Role::Terminal,
            Role::List,
            Role::Table,
            Role::Tree,
        ].into_iter().collect();
        
        let deny_roles = [
            Role::MenuBar,
            Role::Menu,
            Role::MenuItem,
            Role::ToolBar,
            Role::PushButton,
            Role::StatusBar,
            Role::Panel,
            Role::DesktopPane,
        ].into_iter().collect();
        
        Ok(Self { connection, scrollable_roles, deny_roles })
    }
    
    pub fn should_autoscroll(&self, x: i32, y: i32) -> Option<bool> {
        // Get accessible at screen coordinates
        let registry = self.connection.registry();
        let accessible = registry.get_accessible_at_point(x, y, CoordType::Screen).ok()?;
        
        // Walk up the accessibility tree
        let mut current = Some(accessible);
        while let Some(acc) = current {
            let role = acc.role().ok()?;
            
            // Deny takes precedence
            if self.deny_roles.contains(&role) {
                return Some(false);
            }
            
            // Check for scrollable
            if self.scrollable_roles.contains(&role) {
                return Some(true);
            }
            
            // Check if has scroll interface
            if acc.get_interface::<dyn Scrollable>().is_some() {
                return Some(true);
            }
            
            current = acc.parent().ok();
        }
        
        // Unknown - conservative default
        None
    }
}
```

**Pros:**
- Works on both X11 and Wayland
- Most accurate detection (distinguishes browser chrome from page content)
- GTK, Qt, Electron apps all support AT-SPI

**Cons:**
- Requires AT-SPI to be enabled (usually is by default)
- ~10-50ms latency per query
- Some apps have incomplete AT-SPI support

**Dependencies to add:**
```toml
# Cargo.toml
atspi = "0.22"           # AT-SPI D-Bus client
zbus = "4"               # D-Bus library (atspi dependency)
```

### Option B: libei (Emulated Input) for Cursor Position

On Wayland, getting cursor position requires compositor cooperation. Use `libei`:

```rust
// For cursor position on Wayland
use reis::{PendingRequestType, Context, DeviceType};

pub struct WaylandCursorTracker {
    context: Context,
    x: AtomicI32,
    y: AtomicI32,
}

impl WaylandCursorTracker {
    pub fn position(&self) -> (i32, i32) {
        (self.x.load(Ordering::Relaxed), self.y.load(Ordering::Relaxed))
    }
}
```

**Note:** libei requires compositor support (GNOME 45+, KDE Plasma 6+).

### Option C: Compositor-Specific Extensions

For fallback on older compositors:

| Compositor | Method |
|------------|--------|
| GNOME (Mutter) | D-Bus: `org.gnome.Shell.Introspect` |
| KDE (KWin) | D-Bus: `org.kde.KWin` or `org.kde.plasmashell` |
| wlroots-based | `zwlr_foreign_toplevel_management_v1` protocol |
| Hyprland | `hyprctl` CLI or IPC socket |

---

## Phase 4: Wayland Overlay Indicator

### Option A: Layer Shell Protocol - RECOMMENDED

Most Wayland compositors support `zwlr_layer_shell_v1`:

```rust
// src/overlay_wayland.rs

use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    shell::wlr_layer::{
        Layer, LayerShell, LayerSurface, LayerSurfaceConfigure,
    },
    shm::Shm,
};

pub struct WaylandOverlay {
    layer_surface: LayerSurface,
    visible: bool,
}

impl WaylandOverlay {
    pub fn new() -> Result<Self> {
        // Initialize Wayland connection
        let conn = wayland_client::Connection::connect_to_env()?;
        let display = conn.display();
        
        // Get layer shell
        let layer_shell: LayerShell = /* ... */;
        
        // Create layer surface on overlay layer
        let layer_surface = layer_shell.create_layer_surface(
            &compositor,
            &surface,
            Layer::Overlay,  // Always on top
            Some("razerlinux-autoscroll"),
            None,  // All outputs
        );
        
        // Configure for overlay behavior
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer_surface.set_size(32, 32);
        layer_surface.set_anchor(Anchor::empty());  // Floating
        
        Ok(Self { layer_surface, visible: false })
    }
    
    pub fn show_at(&mut self, x: i32, y: i32) {
        self.layer_surface.set_margin(y, 0, 0, x);
        self.visible = true;
        /* ... commit surface ... */
    }
}
```

**Dependencies:**
```toml
wayland-client = "0.31"
wayland-protocols-wlr = "0.3"  # For layer-shell
smithay-client-toolkit = "0.19"
```

### Option B: GTK4 Layer Shell (Simpler)

If using GTK for the main window anyway:

```rust
use gtk4_layer_shell::{Edge, Layer, LayerShell};

let overlay = gtk::Window::new();
gtk4_layer_shell::init_for_window(&overlay);
gtk4_layer_shell::set_layer(&overlay, Layer::Overlay);
gtk4_layer_shell::set_keyboard_mode(&overlay, KeyboardMode::None);
```

### Option C: XDG Desktop Portal (Most Portable)

For sandboxed apps (Flatpak), use portal APIs:

```rust
use ashpd::desktop::screencast::{Screencast, SourceType};
// Limited - mainly for screen capture, not overlays
```

**Fallback:** If no layer-shell support, show indicator in the main Slint window instead (less elegant but works everywhere).

---

## Phase 5: Implementation Roadmap

### Stage 1: Abstraction Layer (Week 1-2)
- [ ] Create `DisplayBackend` trait and runtime detection
- [ ] Refactor `scroll_detect_x11.rs` to implement trait
- [ ] Refactor `overlay.rs` to implement trait  
- [ ] Update `remap.rs` to use abstract backend
- [ ] Add feature flags for X11/Wayland

### Stage 2: AT-SPI Integration (Week 2-3)
- [ ] Add `atspi` crate dependency
- [ ] Implement `ScrollDetectorAtSpi` 
- [ ] Test with GTK, Qt, Electron apps
- [ ] Profile latency and optimize caching

### Stage 3: Wayland Overlay (Week 3-4)
- [ ] Add `smithay-client-toolkit` dependency
- [ ] Implement `WaylandOverlay` with layer-shell
- [ ] Handle multi-monitor scenarios
- [ ] Test on GNOME, KDE, Sway, Hyprland

### Stage 4: Cursor Position (Week 4-5)
- [ ] Integrate libei for GNOME/KDE
- [ ] Add fallback methods for other compositors
- [ ] Handle cases where position unavailable

### Stage 5: Testing & Polish (Week 5-6)
- [ ] Test on major distros (Fedora Wayland, Ubuntu Wayland, Arch + Hyprland)
- [ ] Add graceful degradation for unsupported compositors
- [ ] Update documentation
- [ ] Performance optimization

---

## Cargo.toml Changes

```toml
[features]
default = ["x11", "wayland"]
x11 = ["dep:x11rb"]
wayland = ["dep:wayland-client", "dep:smithay-client-toolkit"]
atspi = ["dep:atspi", "dep:zbus"]

[dependencies]
# Existing X11 support
x11rb = { version = "0.13", features = ["allow-unsafe-code"], optional = true }

# Wayland support
wayland-client = { version = "0.31", optional = true }
wayland-protocols-wlr = { version = "0.3", optional = true }
smithay-client-toolkit = { version = "0.19", optional = true }

# AT-SPI accessibility (works on both X11 and Wayland)
atspi = { version = "0.22", optional = true }
zbus = { version = "4", optional = true }
```

---

## New File Structure

```
src/
├── display_backend/
│   ├── mod.rs           # Backend trait + detection
│   ├── x11.rs           # X11 implementation (existing code refactored)
│   ├── wayland.rs       # Wayland implementation
│   └── null.rs          # Fallback for headless/unknown
├── scroll_detect/
│   ├── mod.rs           # Trait + dispatcher
│   ├── x11.rs           # Current X11 detection
│   ├── atspi.rs         # AT-SPI (cross-platform)
│   └── heuristic.rs     # App-name based fallback
├── overlay/
│   ├── mod.rs           # Trait + dispatcher
│   ├── x11.rs           # Current X11 overlay
│   ├── wayland.rs       # Layer-shell overlay
│   └── fallback.rs      # In-window indicator
└── ... (existing files)
```

---

## Graceful Degradation Strategy

When Wayland features are unavailable:

| Feature | Fallback |
|---------|----------|
| Cursor position | Use evdev relative motion accumulation |
| Scroll detection | Use app-name heuristic only (from `libei` device hints) |
| Overlay | Show indicator in Slint main window corner |
| Layer-shell unavailable | Skip overlay entirely, log warning |

---

## Testing Matrix

| Compositor | Layer Shell | libei | AT-SPI | Notes |
|------------|-------------|-------|--------|-------|
| **GNOME 45+** | ✅ | ✅ | ✅ | Full support |
| **KDE Plasma 6** | ✅ | ✅ | ✅ | Full support |
| **Sway** | ✅ | ❌ | ✅ | No libei, but layer-shell works |
| **Hyprland** | ✅ | ❌ | ✅ | No libei, has IPC fallback |
| **weston** | ❌ | ❌ | ✅ | Limited, use fallback |
| **XWayland apps** | N/A | N/A | ✅ | AT-SPI works |

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| AT-SPI disabled by user | Low | Medium | Use app-name heuristic fallback |
| Old compositor (no layer-shell) | Medium | Low | Use in-window indicator |
| libei not available | Medium | Low | Use evdev motion accumulation |
| App has broken AT-SPI | Medium | Low | Maintain allow/deny lists |
| Performance (AT-SPI latency) | Medium | Medium | Aggressive caching, async queries |

---

## Success Criteria

1. **Autoscroll works on Wayland** - Core feature functional on GNOME, KDE, Sway
2. **Scroll detection accurate** - AT-SPI correctly identifies scrollable areas
3. **Overlay displays correctly** - Layer-shell overlay appears at cursor
4. **No regression on X11** - Existing functionality unchanged
5. **Graceful degradation** - Works (with reduced features) on unsupported compositors

---

## References

- [Wayland Layer Shell Protocol](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)
- [AT-SPI2 Documentation](https://docs.gtk.org/atspi2/)
- [libei Protocol](https://gitlab.freedesktop.org/libinput/libei)
- [Smithay Client Toolkit](https://github.com/Smithay/client-toolkit)
- [atspi-rs Crate](https://crates.io/crates/atspi)
