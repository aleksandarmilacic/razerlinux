//! X11 Display Backend
//!
//! Provides scroll detection and overlay display for X11 sessions.
//!
//! This module uses:
//! - `_NET_WM_WINDOW_TYPE` property for window type detection
//! - `WM_CLASS` property for application identification
//! - XShape extension for click-through overlay windows

use super::{OverlayCommand, OverlayDisplay, ScrollDetector};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::RwLock;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

/// Get cursor position from xdotool (works on X11 and XWayland)
/// Returns (x, y) or None if not available
fn get_xdotool_cursor_position() -> Option<(i32, i32)> {
    let output = Command::new("xdotool")
        .args(["getmouselocation", "--shell"])
        .output()
        .ok()?;
    
    if !output.status.success() {
        warn!("xdotool failed with status: {:?}", output.status);
        return None;
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut x: Option<i32> = None;
    let mut y: Option<i32> = None;
    
    for line in stdout.lines() {
        if let Some(val) = line.strip_prefix("X=") {
            x = val.parse().ok();
        } else if let Some(val) = line.strip_prefix("Y=") {
            y = val.parse().ok();
        }
    }
    
    if let (Some(px), Some(py)) = (x, y) {
        info!("xdotool cursor position: ({}, {})", px, py);
        return Some((px, py));
    }
    
    warn!("xdotool output parsing failed: {}", stdout);
    None
}

/// Get the position of the primary monitor from kscreen-doctor (KDE Plasma)
/// This is needed to compensate for XWayland/KWin coordinate offset on Wayland
fn get_primary_monitor_offset() -> (i32, i32) {
    // Only needed on Wayland
    if std::env::var("WAYLAND_DISPLAY").is_err() {
        return (0, 0);
    }
    
    // Try kscreen-doctor for KDE Plasma
    let output = match Command::new("kscreen-doctor")
        .args(["-o"])
        .output() {
            Ok(o) => o,
            Err(_) => return (0, 0),
        };
    
    if !output.status.success() {
        return (0, 0);
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Look for lines with Geometry after Output lines
    // Format: "Output: 1 DP-3\n  ...\n  Geometry: 1920,432 1920x1080"
    let mut found_primary = false;
    for line in stdout.lines() {
        // On KDE, primary is usually the one with priority 1 or marked
        if line.contains("Output:") && line.contains("DP-3") {
            found_primary = true;
        }
        if found_primary && line.trim().starts_with("Geometry:") {
            // Parse "Geometry: 1920,432 1920x1080"
            let parts: Vec<&str> = line.trim().split_whitespace().collect();
            if parts.len() >= 2 {
                let coords: Vec<&str> = parts[1].split(',').collect();
                if coords.len() >= 2 {
                    let x = coords[0].parse::<i32>().unwrap_or(0);
                    let y = coords[1].parse::<i32>().unwrap_or(0);
                    info!("Primary monitor (DP-3) offset from kscreen-doctor: ({}, {})", x, y);
                    return (x, y);
                }
            }
        }
    }
    
    (0, 0)
}

// Re-export heuristic lists
pub use super::heuristic::{ALLOW_CLASSES, DENY_CLASSES};

/// Cache entry for scroll detection decisions
struct CacheEntry {
    scrollable: bool,
    timestamp: Instant,
}

/// X11 Scroll Detector
///
/// Uses X11 properties to determine if the cursor is over a scrollable area.
pub struct X11ScrollDetector {
    /// X11 connection
    conn: RustConnection,
    /// Root window
    root: Window,
    /// Cached atom values for denied window types
    deny_type_atoms: HashSet<Atom>,
    /// How many parent windows to check for properties
    parent_limit: usize,
    /// If true, unknown windows are NOT scrollable (Windows-like behavior)
    strict_default: bool,
    /// Decision cache to avoid repeated X11 queries
    cache: RwLock<HashMap<(Window, i16, i16), CacheEntry>>,
    /// Cache TTL
    cache_ttl: Duration,
}

impl X11ScrollDetector {
    /// Create a new X11 scroll detector
    pub fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None)
            .context("Failed to connect to X11 display")?;

        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        let deny_type_names = [
            "_NET_WM_WINDOW_TYPE_DESKTOP",
            "_NET_WM_WINDOW_TYPE_DOCK",
            "_NET_WM_WINDOW_TYPE_TOOLBAR",
            "_NET_WM_WINDOW_TYPE_MENU",
            "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU",
            "_NET_WM_WINDOW_TYPE_POPUP_MENU",
            "_NET_WM_WINDOW_TYPE_TOOLTIP",
            "_NET_WM_WINDOW_TYPE_NOTIFICATION",
            "_NET_WM_WINDOW_TYPE_SPLASH",
            "_NET_WM_WINDOW_TYPE_UTILITY",
            "_NET_WM_WINDOW_TYPE_DIALOG",
        ];

        let mut deny_type_atoms = HashSet::new();
        for name in deny_type_names {
            match intern_atom(&conn, name) {
                Ok(atom) => {
                    deny_type_atoms.insert(atom);
                }
                Err(e) => warn!("Failed to intern atom {}: {}", name, e),
            }
        }

        // On Wayland (using XWayland), most apps are native Wayland apps
        // that won't have X11 properties. In this case, default to allowing
        // autoscroll for unknown windows since we can't detect their type.
        let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok() 
            || std::env::var("XDG_SESSION_TYPE").ok().map(|s| s == "wayland").unwrap_or(false);
        let strict_default = !is_wayland;
        
        info!(
            "X11 scroll detector initialized (root: {}, strict_default: {}, wayland: {})",
            root, strict_default, is_wayland
        );

        Ok(Self {
            conn,
            root,
            deny_type_atoms,
            parent_limit: 10,
            strict_default,
            cache: RwLock::new(HashMap::new()),
            cache_ttl: Duration::from_millis(150),
        })
    }

    /// Cache a detection result
    fn cache_result(&self, key: (Window, i16, i16), scrollable: bool) {
        if let Ok(mut cache) = self.cache.write() {
            // Prune old entries periodically
            if cache.len() > 100 {
                let now = Instant::now();
                cache.retain(|_, v| now.duration_since(v.timestamp) < self.cache_ttl * 2);
            }

            cache.insert(
                key,
                CacheEntry {
                    scrollable,
                    timestamp: Instant::now(),
                },
            );
        }
    }
}

impl ScrollDetector for X11ScrollDetector {
    fn should_autoscroll(&self) -> bool {
        // Get deepest window under pointer
        let (deepest, root_x, root_y) = match deepest_window_under_pointer(&self.conn, self.root) {
            Ok(result) => result,
            Err(e) => {
                warn!("Failed to get window under pointer: {}", e);
                return !self.strict_default;
            }
        };

        // Check cache first (key by window and coarse position)
        let cache_key = (deepest, root_x >> 4, root_y >> 4);
        {
            if let Ok(cache) = self.cache.read() {
                if let Some(entry) = cache.get(&cache_key) {
                    if entry.timestamp.elapsed() < self.cache_ttl {
                        return entry.scrollable;
                    }
                }
            }
        }

        // Get parent chain for property lookup
        let chain = match parent_chain(&self.conn, deepest, self.parent_limit) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to get parent chain: {}", e);
                return !self.strict_default;
            }
        };

        // 1) Deny by window type (check all parents)
        for &w in &chain {
            if let Ok(types) = get_window_type_atoms(&self.conn, w) {
                if types.iter().any(|a| self.deny_type_atoms.contains(a)) {
                    debug!("Denied by window type for window {:?}", w);
                    self.cache_result(cache_key, false);
                    return false;
                }
            }
        }

        // 2) Find WM_CLASS in parent chain
        let mut found_class: Option<String> = None;
        for &w in &chain {
            if let Ok(Some((_instance, class))) = get_wm_class(&self.conn, w) {
                found_class = Some(class.to_lowercase());
                break;
            }
        }

        // 3) Deny by WM_CLASS
        if let Some(ref class) = found_class {
            if DENY_CLASSES.iter().any(|d| class.contains(*d)) {
                debug!("Denied by WM_CLASS: {}", class);
                self.cache_result(cache_key, false);
                return false;
            }
        }

        // 4) Allow by WM_CLASS
        if let Some(ref class) = found_class {
            if ALLOW_CLASSES.iter().any(|a| class.contains(*a)) {
                debug!("Allowed by WM_CLASS: {}", class);
                self.cache_result(cache_key, true);
                return true;
            }
        }

        // 5) Strict default: unknown = NOT scrollable
        debug!(
            "Unknown window class {:?}, strict_default={}",
            found_class, self.strict_default
        );
        let result = !self.strict_default;
        self.cache_result(cache_key, result);
        result
    }

    fn cursor_position(&self) -> Option<(i32, i32)> {
        match self.conn.query_pointer(self.root) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => Some((reply.root_x as i32, reply.root_y as i32)),
                Err(_) => None,
            },
            Err(_) => None,
        }
    }

    fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }
}

/// X11 Overlay Display
///
/// Shows a Windows-style autoscroll indicator using an X11 overlay window.
pub struct X11Overlay {
    sender: Sender<OverlayCommand>,
    thread: Option<thread::JoinHandle<()>>,
}

impl X11Overlay {
    /// Start the X11 overlay system
    pub fn start() -> Result<Self> {
        let (tx, rx) = mpsc::channel();

        let thread = thread::spawn(move || {
            info!("X11 overlay thread starting...");
            match run_overlay_loop(rx) {
                Ok(()) => info!("X11 overlay thread exited normally"),
                Err(e) => error!("X11 overlay thread error: {:#}", e),
            }
        });

        info!("X11 overlay started");
        Ok(Self {
            sender: tx,
            thread: Some(thread),
        })
    }
}

impl OverlayDisplay for X11Overlay {
    fn sender(&self) -> Sender<OverlayCommand> {
        self.sender.clone()
    }

    fn show(&self) {
        // Note: Uses (0,0) as fallback - callers should use sender directly with position
        let _ = self.sender.send(OverlayCommand::Show(0, 0));
    }

    fn hide(&self) {
        let _ = self.sender.send(OverlayCommand::Hide);
    }

    fn shutdown(&mut self) {
        let _ = self.sender.send(OverlayCommand::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for X11Overlay {
    fn drop(&mut self) {
        let _ = self.sender.send(OverlayCommand::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

// ============================================================================
// X11 Helper Functions
// ============================================================================

/// Size of the overlay indicator (pixels)
const INDICATOR_SIZE: u16 = 32;

/// Get the deepest window under the pointer using QueryPointer loop
fn deepest_window_under_pointer<C: Connection>(
    conn: &C,
    root: Window,
) -> Result<(Window, i16, i16)> {
    let mut w = root;

    loop {
        let qp = conn.query_pointer(w)?.reply()?;
        let x = qp.root_x;
        let y = qp.root_y;

        if qp.child == 0 {
            return Ok((w, x, y));
        }
        w = qp.child;
    }
}

/// Walk up the parent chain from a window
fn parent_chain<C: Connection>(conn: &C, mut w: Window, limit: usize) -> Result<Vec<Window>> {
    let mut out = Vec::with_capacity(limit + 1);
    for _ in 0..=limit {
        out.push(w);
        let qt = conn.query_tree(w)?.reply()?;
        if qt.parent == 0 || qt.parent == w {
            break;
        }
        w = qt.parent;
    }
    Ok(out)
}

/// Intern an X11 atom by name
fn intern_atom<C: Connection>(conn: &C, name: &str) -> Result<Atom> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

/// Get `_NET_WM_WINDOW_TYPE` atoms for a window
fn get_window_type_atoms<C: Connection>(conn: &C, w: Window) -> Result<Vec<Atom>> {
    let prop_atom = intern_atom(conn, "_NET_WM_WINDOW_TYPE")?;
    let prop = conn
        .get_property(false, w, prop_atom, AtomEnum::ATOM, 0, 64)?
        .reply()?;

    Ok(prop.value32().map(|it| it.collect()).unwrap_or_default())
}

/// Get WM_CLASS property (instance, class) for a window
fn get_wm_class<C: Connection>(conn: &C, w: Window) -> Result<Option<(String, String)>> {
    let prop = conn
        .get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)?
        .reply()?;

    if prop.value.is_empty() {
        return Ok(None);
    }

    let parts: Vec<&[u8]> = prop
        .value
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .collect();

    let instance = parts
        .first()
        .map(|p| String::from_utf8_lossy(p).to_string())
        .unwrap_or_default();
    let class = parts
        .get(1)
        .map(|p| String::from_utf8_lossy(p).to_string())
        .unwrap_or_default();
    Ok(Some((instance, class)))
}

/// Run the overlay event loop
fn run_overlay_loop(rx: Receiver<OverlayCommand>) -> Result<()> {
    use x11rb::protocol::shape::{self, SK};

    // Connect to X11
    let (conn, screen_num) =
        x11rb::connect(None).context("Failed to connect to X11 display")?;

    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let depth = screen.root_depth;

    // Create the overlay window - position at cursor location initially
    // Use override_redirect to bypass window manager placement
    let win = conn.generate_id()?;

    // Check if we're on Wayland - we may need to adjust coordinates
    let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();

    let values = CreateWindowAux::new()
        .override_redirect(1)  // Must use this to control exact position
        .save_under(1)
        .backing_store(BackingStore::ALWAYS)
        .background_pixmap(x11rb::NONE)
        .border_pixel(screen.white_pixel)
        .event_mask(EventMask::EXPOSURE);

    conn.create_window(
        depth,
        win,
        root,
        0,
        0,
        INDICATOR_SIZE,
        INDICATOR_SIZE,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &values,
    )?;

    // Set window type to UTILITY so KWin treats it correctly
    let wm_window_type = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE")?.reply()?.atom;
    let wm_type_utility = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_UTILITY")?.reply()?.atom;
    conn.change_property(
        PropMode::REPLACE,
        win,
        wm_window_type,
        AtomEnum::ATOM,
        32,
        1,
        &wm_type_utility.to_ne_bytes(),
    )?;

    // Set window to skip taskbar and pager
    let wm_state = conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;
    let state_above = conn.intern_atom(false, b"_NET_WM_STATE_ABOVE")?.reply()?.atom;
    let state_skip_taskbar = conn.intern_atom(false, b"_NET_WM_STATE_SKIP_TASKBAR")?.reply()?.atom;
    let state_skip_pager = conn.intern_atom(false, b"_NET_WM_STATE_SKIP_PAGER")?.reply()?.atom;
    let states = [state_above, state_skip_taskbar, state_skip_pager];
    let states_bytes: Vec<u8> = states.iter().flat_map(|a| a.to_ne_bytes()).collect();
    conn.change_property(
        PropMode::REPLACE,
        win,
        wm_state,
        AtomEnum::ATOM,
        32,
        3,
        &states_bytes,
    )?;

    // Make the window click-through using XShape extension (empty INPUT region)
    // Note: We only set INPUT shape here to make it click-through
    // BOUNDING shape is set during draw_indicator to match the actual content
    let empty_region: &[Rectangle] = &[];
    shape::rectangles(
        &conn,
        shape::SO::SET,
        SK::INPUT,
        ClipOrdering::UNSORTED,
        win,
        0,
        0,
        empty_region,
    )?;

    // Create graphics contexts
    let gc = conn.generate_id()?;
    let gc_values = CreateGCAux::new()
        .foreground(screen.white_pixel)
        .background(screen.black_pixel)
        .line_width(2);
    conn.create_gc(gc, win, &gc_values)?;

    let gc_fill = conn.generate_id()?;
    let gc_fill_values = CreateGCAux::new()
        .foreground(0x00AA00)
        .background(screen.black_pixel);
    conn.create_gc(gc_fill, win, &gc_fill_values)?;

    conn.flush()?;

    info!("X11 overlay window created (is_wayland={})", is_wayland);

    let mut visible = false;
    let mut current_dx: f32 = 0.0;
    let mut current_dy: f32 = 0.0;

    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(OverlayCommand::Show(cursor_x, cursor_y)) => {
                // Use the tracked cursor position passed from the remapper
                // On Wayland, XWayland doesn't see our uinput movements, so query_pointer 
                // returns stale values. The remapper tracks actual evdev REL_X/REL_Y.
                let (px, py) = if cursor_x != 0 || cursor_y != 0 {
                    // Use provided tracked position
                    info!("X11 overlay: using tracked position ({}, {})", cursor_x, cursor_y);
                    (cursor_x, cursor_y)
                } else {
                    // Fallback to query_pointer (works on native X11)
                    if let Ok(reply) = conn.query_pointer(root) {
                        if let Ok(pointer) = reply.reply() {
                            info!("X11 overlay: query_pointer returned ({}, {})", pointer.root_x, pointer.root_y);
                            (pointer.root_x as i32, pointer.root_y as i32)
                        } else {
                            info!("X11 overlay: query_pointer failed");
                            (100, 100) // Safe fallback
                        }
                    } else {
                        info!("X11 overlay: query_pointer error");
                        (100, 100) // Safe fallback
                    }
                };
                
                // On Wayland/XWayland, KWin places override-redirect windows with an offset.
                // The cursor coordinates from xdotool are global, but KWin subtracts the
                // primary monitor offset when placing windows. So we need to keep the
                // original coordinates (don't adjust) since xdotool gives us the right values.
                // 
                // Actually, testing showed the window appears 1 screen to the LEFT,
                // meaning KWin is NOT offsetting - the issue is something else.
                // Let's try NOT adjusting and see what happens.
                let x = px as i16 - (INDICATOR_SIZE as i16 / 2);
                let y = py as i16 - (INDICATOR_SIZE as i16 / 2);
                
                info!("X11 overlay: positioning window at ({}, {}) for cursor at ({}, {})", x, y, px, py);

                conn.configure_window(
                    win,
                    &ConfigureWindowAux::new().x(x as i32).y(y as i32),
                )?;
                conn.map_window(win)?;
                conn.flush()?;

                visible = true;
                current_dx = 0.0;
                current_dy = 0.0;

                draw_indicator(&conn, win, gc, 0.0, 0.0)?;

                // Query actual window geometry to verify placement
                if let Ok(geom) = conn.get_geometry(win) {
                    if let Ok(geom) = geom.reply() {
                        info!("X11 overlay: actual geometry after placement: x={}, y={}, w={}, h={}", 
                              geom.x, geom.y, geom.width, geom.height);
                    }
                }
                // Also translate to root coordinates
                if let Ok(trans) = conn.translate_coordinates(win, root, 0, 0) {
                    if let Ok(trans) = trans.reply() {
                        info!("X11 overlay: translated root coords: x={}, y={}", trans.dst_x, trans.dst_y);
                    }
                }

                info!("X11 overlay shown at ({}, {})", x, y);
            }
            Ok(OverlayCommand::Hide) => {
                if visible {
                    conn.unmap_window(win)?;
                    conn.flush()?;
                    visible = false;
                    current_dx = 0.0;
                    current_dy = 0.0;
                    info!("X11 overlay hidden");
                }
            }
            Ok(OverlayCommand::UpdateDirection(dx, dy)) => {
                if visible {
                    let dx_changed = (dx - current_dx).abs() > 0.2;
                    let dy_changed = (dy - current_dy).abs() > 0.2;
                    if dx_changed || dy_changed {
                        current_dx = dx;
                        current_dy = dy;
                        draw_indicator(&conn, win, gc, dx, dy)?;
                    }
                }
            }
            Ok(OverlayCommand::Shutdown) => {
                info!("X11 overlay shutting down");
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Process X11 events if any
                while let Some(event) = conn.poll_for_event()? {
                    if let x11rb::protocol::Event::Expose(_) = event {
                        if visible {
                            draw_indicator(&conn, win, gc, current_dx, current_dy)?;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                info!("X11 overlay channel disconnected");
                break;
            }
        }
    }

    // Cleanup
    conn.destroy_window(win)?;
    conn.free_gc(gc)?;
    conn.free_gc(gc_fill)?;
    conn.flush()?;

    Ok(())
}

/// Draw the autoscroll indicator
fn draw_indicator<C: Connection>(
    conn: &C,
    win: Window,
    gc: Gcontext,
    dx: f32,
    dy: f32,
) -> Result<()> {
    use x11rb::protocol::shape::{self, SK};

    let size = INDICATOR_SIZE as i16;
    let center = size / 2;

    // Set bounding shape
    let icon_radius = 14i16;
    let bounding_rects = [
        Rectangle {
            x: center - icon_radius + 4,
            y: center - icon_radius,
            width: (icon_radius * 2 - 8) as u16,
            height: (icon_radius * 2) as u16,
        },
        Rectangle {
            x: center - icon_radius + 2,
            y: center - icon_radius + 2,
            width: (icon_radius * 2 - 4) as u16,
            height: (icon_radius * 2 - 4) as u16,
        },
        Rectangle {
            x: center - icon_radius,
            y: center - icon_radius + 4,
            width: (icon_radius * 2) as u16,
            height: (icon_radius * 2 - 8) as u16,
        },
    ];
    shape::rectangles(
        conn,
        shape::SO::SET,
        SK::BOUNDING,
        ClipOrdering::UNSORTED,
        win,
        0,
        0,
        &bounding_rects,
    )?;

    // Fill background
    conn.change_gc(gc, &ChangeGCAux::new().foreground(0x333333))?;
    conn.poly_fill_rectangle(
        win,
        gc,
        &[Rectangle {
            x: 0,
            y: 0,
            width: INDICATOR_SIZE,
            height: INDICATOR_SIZE,
        }],
    )?;

    // Reset to white
    conn.change_gc(gc, &ChangeGCAux::new().foreground(0xFFFFFF))?;

    // Draw center dot
    let dot_radius = 3i16;
    conn.poly_fill_arc(
        win,
        gc,
        &[Arc {
            x: center - dot_radius,
            y: center - dot_radius,
            width: (dot_radius * 2) as u16,
            height: (dot_radius * 2) as u16,
            angle1: 0,
            angle2: 360 * 64,
        }],
    )?;

    // Arrow positioning
    let arrow_offset = 9i16;
    let arrow_size = 4i16;

    let show_up = dy < -0.3;
    let show_down = dy > 0.3;
    let show_left = dx < -0.3;
    let show_right = dx > 0.3;
    let show_all = !show_up && !show_down && !show_left && !show_right;

    // Up arrow
    if show_up || show_all {
        let tip_y = center - arrow_offset - arrow_size;
        let base_y = center - arrow_offset + 1;
        let points = [
            Point { x: center, y: tip_y },
            Point {
                x: center - arrow_size,
                y: base_y,
            },
            Point {
                x: center + arrow_size,
                y: base_y,
            },
        ];
        conn.fill_poly(win, gc, PolyShape::CONVEX, CoordMode::ORIGIN, &points)?;
    }

    // Down arrow
    if show_down || show_all {
        let tip_y = center + arrow_offset + arrow_size;
        let base_y = center + arrow_offset - 1;
        let points = [
            Point { x: center, y: tip_y },
            Point {
                x: center - arrow_size,
                y: base_y,
            },
            Point {
                x: center + arrow_size,
                y: base_y,
            },
        ];
        conn.fill_poly(win, gc, PolyShape::CONVEX, CoordMode::ORIGIN, &points)?;
    }

    // Left arrow
    if show_left || show_all {
        let tip_x = center - arrow_offset - arrow_size;
        let base_x = center - arrow_offset + 1;
        let points = [
            Point { x: tip_x, y: center },
            Point {
                x: base_x,
                y: center - arrow_size,
            },
            Point {
                x: base_x,
                y: center + arrow_size,
            },
        ];
        conn.fill_poly(win, gc, PolyShape::CONVEX, CoordMode::ORIGIN, &points)?;
    }

    // Right arrow
    if show_right || show_all {
        let tip_x = center + arrow_offset + arrow_size;
        let base_x = center + arrow_offset - 1;
        let points = [
            Point { x: tip_x, y: center },
            Point {
                x: base_x,
                y: center - arrow_size,
            },
            Point {
                x: base_x,
                y: center + arrow_size,
            },
        ];
        conn.fill_poly(win, gc, PolyShape::CONVEX, CoordMode::ORIGIN, &points)?;
    }

    conn.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_lists_lowercase() {
        for class in DENY_CLASSES {
            assert_eq!(
                class,
                &class.to_lowercase(),
                "Deny class should be lowercase"
            );
        }
        for class in ALLOW_CLASSES {
            assert_eq!(
                class,
                &class.to_lowercase(),
                "Allow class should be lowercase"
            );
        }
    }
}
