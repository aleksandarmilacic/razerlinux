//! Software button remapping (evdev grab + uinput virtual device)

use crate::display_backend::{DisplayBackend, OverlayCommand, ScrollDetector};
use anyhow::{Context, Result};
use evdev::{AttributeSet, Device, EventType, InputEvent, InputEventKind, Key, uinput::VirtualDeviceBuilder};
use std::collections::BTreeMap;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{info, warn, debug};

#[derive(Debug, Clone, Default)]
pub struct RemapConfig {
    pub source_device: Option<String>,
    pub mappings: BTreeMap<u16, MappingTarget>,
    /// Enable Windows-style autoscroll (middle click to enter scroll mode)
    pub autoscroll_enabled: bool,
}

/// Extended config passed to remapper thread (includes non-Clone items)
pub struct RemapConfigExt {
    pub config: RemapConfig,
    pub overlay_sender: Option<Sender<OverlayCommand>>,
    /// Macros available for execution (cloned from MacroManager at start)
    pub macros: std::collections::HashMap<u32, crate::profile::Macro>,
}

#[derive(Debug, Clone, Default)]
pub struct MappingTarget {
    pub base: u16,
    pub mods: Modifiers,
}

#[derive(Debug, Clone, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Modifiers {
    pub fn to_key_codes(&self) -> impl Iterator<Item = u16> + '_ {
        let mut codes: [u16; 4] = [0; 4];
        let mut len = 0;
        if self.ctrl {
            codes[len] = Key::KEY_LEFTCTRL.0;
            len += 1;
        }
        if self.alt {
            codes[len] = Key::KEY_LEFTALT.0;
            len += 1;
        }
        if self.shift {
            codes[len] = Key::KEY_LEFTSHIFT.0;
            len += 1;
        }
        if self.meta {
            codes[len] = Key::KEY_LEFTMETA.0;
            len += 1;
        }

        codes.into_iter().take(len)
    }
}

/// Get cursor position from KWin (Plasma Wayland) using a script
/// This is the only reliable method on Wayland since xdotool returns stale XWayland positions
fn get_cursor_position_kwin() -> Option<(i32, i32)> {
    use std::io::Write;
    use std::process::Command;
    
    // Check if we're on Wayland with KDE
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    
    if session_type != "wayland" || !desktop.to_lowercase().contains("kde") {
        debug!("KWin cursor: Not on KDE Wayland (session={}, desktop={})", session_type, desktop);
        return None;
    }
    
    // Create a unique marker for this query
    let marker = format!("RAZERLINUX_CURSOR_{}", std::process::id());
    
    // Create temporary script
    let script_content = format!(
        "var pos = workspace.cursorPos;\nprint(\"{}: \" + pos.x + \",\" + pos.y);",
        marker
    );
    
    let script_path = "/tmp/razerlinux_cursor.js";
    if let Ok(mut file) = std::fs::File::create(script_path) {
        if file.write_all(script_content.as_bytes()).is_err() {
            debug!("KWin cursor: Failed to write script file");
            return None;
        }
    } else {
        debug!("KWin cursor: Failed to create script file");
        return None;
    }
    
    // Load the script via qdbus6
    let load_result = Command::new("qdbus6")
        .args(["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.loadScript", script_path])
        .output();
        
    match &load_result {
        Ok(output) => {
            if !output.status.success() {
                debug!("KWin cursor: qdbus6 loadScript failed with status {:?}", output.status);
                let _ = std::fs::remove_file(script_path);
                return None;
            }
            debug!("KWin cursor: Script loaded successfully");
        }
        Err(e) => {
            debug!("KWin cursor: qdbus6 command failed: {}", e);
            let _ = std::fs::remove_file(script_path);
            return None;
        }
    }
    
    // Start the scripting system to execute the script
    let _ = Command::new("qdbus6")
        .args(["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.start"])
        .output();
    
    // Give the script a moment to execute (50ms is enough based on testing)
    std::thread::sleep(std::time::Duration::from_millis(50));
    
    // Read the output from journalctl
    if let Ok(output) = Command::new("journalctl")
        .args(["--user", "-n", "30", "--since", "10 seconds ago", "--no-pager", "-o", "cat"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        debug!("KWin cursor: Looking for marker '{}' in {} journal lines", marker, stdout.lines().count());
        for line in stdout.lines().rev() {
            // KWin script output format: "js: MARKER: x,y"
            if line.contains(&marker) {
                // Try to extract coordinates after the marker
                if let Some(pos) = line.find(&format!("{}: ", marker)) {
                    let after_marker = &line[pos + marker.len() + 2..]; // +2 for ": "
                    let parts: Vec<&str> = after_marker.split(',').collect();
                    if parts.len() >= 2 {
                        if let (Ok(x), Ok(y)) = (parts[0].trim().parse::<i32>(), parts[1].trim().parse::<i32>()) {
                            info!("KWin cursor: Got position from KWin script: ({}, {})", x, y);
                            let _ = std::fs::remove_file(script_path);
                            return Some((x, y));
                        }
                    }
                }
            }
        }
        debug!("KWin cursor: Marker not found in journal output");
    } else {
        debug!("KWin cursor: journalctl command failed");
    }
    
    // Clean up
    let _ = std::fs::remove_file(script_path);
    None
}

/// Information about a detected Razer input interface
#[derive(Debug, Clone)]
pub struct RazerInputInterface {
    pub path: PathBuf,
    pub name: String,
    pub has_mouse_buttons: bool,
    pub has_keyboard_keys: bool,
    pub num_buttons: usize,
    pub num_keys: usize,
}

/// List all Razer input interfaces for debugging purposes.
/// The Naga Trinity exposes multiple interfaces:
///   - input0: Mouse (5 buttons)
///   - input1: Keyboard (side panel keys come through here)
///   - input2: Another keyboard interface
pub fn list_razer_input_interfaces() -> Vec<RazerInputInterface> {
    let mut interfaces = Vec::new();
    
    for (path, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or_default().to_string();
        let name_lower = name.to_ascii_lowercase();
        let is_razer = name_lower.contains("razer") || name_lower.contains("naga");
        
        if !is_razer {
            continue;
        }
        
        let keys = dev.supported_keys();
        
        let has_mouse_buttons = keys
            .as_ref()
            .map(|k| {
                k.contains(Key::BTN_LEFT)
                    || k.contains(Key::BTN_RIGHT)
                    || k.contains(Key::BTN_MIDDLE)
                    || k.contains(Key::BTN_SIDE)
                    || k.contains(Key::BTN_EXTRA)
            })
            .unwrap_or(false);
            
        // Count button codes (0x110-0x15F range)
        let num_buttons = keys.as_ref().map(|k| {
            k.iter().filter(|key| key.code() >= 0x110 && key.code() < 0x160).count()
        }).unwrap_or(0);
        
        // Count keyboard keys (0x00-0xFF range)
        let num_keys = keys.as_ref().map(|k| {
            k.iter().filter(|key| key.code() < 0x100).count()
        }).unwrap_or(0);
        
        let has_keyboard_keys = num_keys > 0;
        
        interfaces.push(RazerInputInterface {
            path,
            name,
            has_mouse_buttons,
            has_keyboard_keys,
            num_buttons,
            num_keys,
        });
    }
    
    interfaces
}

pub struct Remapper {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl Remapper {
    pub fn start(
        config: RemapConfig, 
        overlay_sender: Option<Sender<OverlayCommand>>,
        macros: std::collections::HashMap<u32, crate::profile::Macro>,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();

        let ext_config = RemapConfigExt {
            config,
            overlay_sender,
            macros,
        };

        let join = thread::spawn(move || {
            if let Err(e) = run_remapper_loop(stop_thread, ext_config) {
                warn!("remapper stopped: {e:#}");
            }
        });

        Ok(Self {
            stop,
            join: Some(join),
        })
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Remapper {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

fn run_remapper_loop(stop: Arc<AtomicBool>, ext_config: RemapConfigExt) -> Result<()> {
    let config = ext_config.config;
    let overlay_sender = ext_config.overlay_sender;
    let macros = ext_config.macros;
    
    // Initialize scroll detector for Windows-like autoscroll behavior
    // This detects if the cursor is over a scrollable area (not desktop/dock/menu)
    // Uses the display backend abstraction to support both X11 and Wayland
    let scroll_detector: Option<Box<dyn ScrollDetector>> = if config.autoscroll_enabled {
        let backend = DisplayBackend::new();
        match backend.create_scroll_detector() {
            Some(detector) => {
                info!("Scroll detector initialized for {}", backend.display_server().name());
                Some(detector)
            }
            None => {
                warn!("No scroll detector available - autoscroll will work everywhere");
                None
            }
        }
    } else {
        None
    };
    
    // Find ALL Razer keyboard interfaces - the Naga Trinity sends side button keys
    // through multiple interfaces (event9 AND event11), so we need to grab them all
    let source_paths = select_all_razer_keyboard_devices(&config.source_device);
    
    if source_paths.is_empty() {
        anyhow::bail!("No suitable Razer keyboard interfaces found for remapping");
    }

    // IMPORTANT: Get initial cursor position BEFORE grabbing devices!
    // Once we grab evdev devices, xdotool/X11 won't see hardware mouse movements anymore
    // (especially on Wayland/XWayland where the position gets "frozen")
    let (initial_cursor_x, initial_cursor_y): (i32, i32) = {
        // On Wayland/KDE, try KWin script first - this is the ONLY reliable method
        // xdotool returns stale positions on XWayland
        if let Some(pos) = get_cursor_position_kwin() {
            info!("Initial cursor position from KWin (BEFORE grab): ({}, {})", pos.0, pos.1);
            pos
        } else if let Ok(output) = std::process::Command::new("xdotool")
            .args(["getmouselocation", "--shell"])
            .output()
        {
            // Fallback to xdotool (works on X11, may be stale on XWayland)
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut x = 0i32;
            let mut y = 0i32;
            for line in stdout.lines() {
                if let Some(val) = line.strip_prefix("X=") {
                    x = val.parse().unwrap_or(0);
                } else if let Some(val) = line.strip_prefix("Y=") {
                    y = val.parse().unwrap_or(0);
                }
            }
            info!("Initial cursor position from xdotool (BEFORE grab): ({}, {})", x, y);
            (x, y)
        } else if let Some(ref detector) = scroll_detector {
            // Fallback to scroll detector
            let pos = detector.cursor_position().unwrap_or((0, 0));
            info!("Initial cursor position from scroll detector: ({}, {})", pos.0, pos.1);
            pos
        } else {
            warn!("Could not get initial cursor position - overlay may appear at wrong location");
            (0, 0)
        }
    };

    info!("Starting remapper on {} device(s): {:?}", source_paths.len(), source_paths);

    // Open and grab all source devices
    let mut devices: Vec<Device> = Vec::new();
    let mut all_keys: AttributeSet<Key> = AttributeSet::new();
    let mut all_rel: AttributeSet<evdev::RelativeAxisType> = AttributeSet::new();
    
    for source_path in &source_paths {
        let mut dev = Device::open(source_path)
            .with_context(|| format!("Failed to open evdev device: {source_path:?}"))?;

        set_nonblocking(&dev).context("Failed to set evdev device non-blocking")?;

        // Grab the device so the original events don't reach the system.
        dev.grab().with_context(|| format!("Failed to grab evdev device: {source_path:?}"))?;
        
        info!("Grabbed device: {:?}", source_path);

        // Collect key capabilities from all devices
        if let Some(src_keys) = dev.supported_keys() {
            let key_count = src_keys.iter().count();
            info!("  -> {} key capabilities", key_count);
            for k in src_keys.iter() {
                all_keys.insert(k);
            }
        }
        
        // Collect relative axis capabilities from ALL devices (for scroll wheel, mouse movement)
        if let Some(rel) = dev.supported_relative_axes() {
            let axis_count = rel.iter().count();
            info!("  -> {} relative axes (scroll wheel, mouse motion)", axis_count);
            for axis in rel.iter() {
                all_rel.insert(axis);
            }
        }
        
        devices.push(dev);
    }
    
    // Add target keys to capabilities (except special scroll codes)
    for target in config.mappings.values() {
        // Skip special scroll codes - they are REL events, not KEY events
        if target.base == 280 || target.base == 281 {
            continue;
        }
        all_keys.insert(Key::new(target.base));
        for m in target.mods.to_key_codes() {
            all_keys.insert(Key::new(m));
        }
    }
    
    // Always add BTN_FORWARD and BTN_BACK in case they're used as targets
    all_keys.insert(Key::BTN_FORWARD);
    all_keys.insert(Key::BTN_BACK);

    // Build virtual device
    let mut vbuilder = VirtualDeviceBuilder::new().context("Failed to create uinput builder")?;
    vbuilder = vbuilder.name(&"RazerLinux Virtual Device");

    vbuilder = vbuilder
        .with_keys(&all_keys)
        .context("Failed to set key capabilities")?;
    
    // Add relative axes if any were found (for scroll wheel, mouse movement)
    let has_rel_axes = all_rel.iter().next().is_some();
    if has_rel_axes {
        info!("Virtual device will have relative axes (scroll wheel, mouse movement)");
        vbuilder = vbuilder
            .with_relative_axes(&all_rel)
            .context("Failed to set relative axis capabilities")?;
    } else {
        warn!("No relative axes found - scroll wheel may not work!");
    }

    let mut vdev = vbuilder.build().context("Failed to build uinput device")?;
    
    info!("Virtual device created, processing events from {} source(s)...", devices.len());
    info!("Active mappings: {:?}", config.mappings);
    info!("Autoscroll enabled: {}", config.autoscroll_enabled);

    // Autoscroll state - Windows style with two modes:
    // 1. Hold Mode: Press and hold middle button, release to exit
    // 2. Toggle Mode: Short click to enter, click any button to exit
    let mut autoscroll_active = false;
    let mut autoscroll_toggle_mode = false;  // true = toggle mode (click to exit), false = hold mode
    let mut autoscroll_moved = false;  // Track if mouse moved during autoscroll
    let mut middle_press_time: Option<Instant> = None;  // Track when middle button was pressed
    let mut middle_passthrough = false;  // Track if middle press was passed through (non-scrollable area)
    let mut anchor_x: i32 = 0;  // Anchor point X (where middle-click happened)
    let mut anchor_y: i32 = 0;  // Anchor point Y
    let mut cursor_x: i32 = 0;  // Current cursor position X (relative to anchor)
    let mut cursor_y: i32 = 0;  // Current cursor position Y (relative to anchor)
    
    // Absolute cursor position tracking (for overlay positioning)
    // Use the position we captured BEFORE grabbing devices
    let (mut abs_cursor_x, mut abs_cursor_y) = (initial_cursor_x, initial_cursor_y);
    // Screen bounds for clamping (approximate - could be queried from X11)
    const SCREEN_WIDTH: i32 = 7680;   // Max reasonable multi-monitor width
    const SCREEN_HEIGHT: i32 = 4320;  // Max reasonable multi-monitor height
    
    let mut scroll_tick_counter: u32 = 0;  // For throttling scroll events
    let mut autoscroll_start_time: Option<Instant> = None;  // When autoscroll was activated
    const SCROLL_DEAD_ZONE: i32 = 15;  // Pixels from anchor before scrolling starts
    const SCROLL_TICK_INTERVAL: u32 = 3;  // Emit scroll every N movement events
    const DIRECTION_UPDATE_INTERVAL: u32 = 12;  // Update overlay direction every N events
    const QUICK_CLICK_THRESHOLD_MS: u64 = 150;  // Max ms for quick click (pass through for links etc)
    const TOGGLE_MODE_THRESHOLD_MS: u64 = 400;  // Max ms for toggle mode (between quick click and hold)
    const SCROLL_GRACE_PERIOD_MS: u64 = 0;  // No delay - scroll detection starts immediately
    const BTN_MIDDLE: u16 = 274;
    const REL_WHEEL: u16 = 8;
    const REL_HWHEEL: u16 = 6;
    
    // Speed zones for gradual acceleration (distance -> scroll speed)
    // Zone 1: 15-50px = speed 1 (slow)
    // Zone 2: 50-100px = speed 2 (medium-slow) 
    // Zone 3: 100-150px = speed 3 (medium)
    // Zone 4: 150-200px = speed 4 (medium-fast)
    // Zone 5: 200-300px = speed 5 (fast)
    // Zone 6: 300+px = speed 6 (very fast)
    fn calculate_scroll_speed(distance: i32, dead_zone: i32) -> i32 {
        let d = distance.abs();
        if d <= dead_zone {
            0
        } else if d <= 50 {
            1  // Slow
        } else if d <= 100 {
            2  // Medium-slow
        } else if d <= 150 {
            3  // Medium
        } else if d <= 200 {
            4  // Medium-fast
        } else if d <= 300 {
            5  // Fast
        } else {
            6  // Very fast
        }
    }

    // Get current cursor position using xdotool (works on Wayland/XWayland)
    // Note: This only works reliably before device grab or after events are emitted to uinput
    fn get_cursor_position_xdotool() -> (i32, i32) {
        // Small delay to let X server process any pending uinput events
        std::thread::sleep(std::time::Duration::from_millis(5));
        if let Ok(output) = std::process::Command::new("xdotool")
            .args(["getmouselocation", "--shell"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut x = 0i32;
            let mut y = 0i32;
            for line in stdout.lines() {
                if let Some(val) = line.strip_prefix("X=") {
                    x = val.parse().unwrap_or(0);
                } else if let Some(val) = line.strip_prefix("Y=") {
                    y = val.parse().unwrap_or(0);
                }
            }
            (x, y)
        } else {
            (0, 0)
        }
    }

    while !stop.load(Ordering::Relaxed) {
        let mut had_events = false;
        
        for dev in &mut devices {
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        had_events = true;
                        
                        // Track absolute cursor position FIRST (before any other processing)
                        // This ensures we have up-to-date position when middle button is pressed
                        if let InputEventKind::RelAxis(axis) = ev.kind() {
                            match axis {
                                evdev::RelativeAxisType::REL_X => {
                                    abs_cursor_x = (abs_cursor_x + ev.value()).clamp(0, SCREEN_WIDTH);
                                }
                                evdev::RelativeAxisType::REL_Y => {
                                    abs_cursor_y = (abs_cursor_y + ev.value()).clamp(0, SCREEN_HEIGHT);
                                }
                                _ => {}
                            }
                        }
                        
                        // Handle autoscroll if enabled
                        if config.autoscroll_enabled {
                            // Check for middle button press/release
                            if let InputEventKind::Key(key) = ev.kind() {
                                if key.code() == BTN_MIDDLE {
                                    if ev.value() == 1 {
                                        // Middle button pressed
                                        if autoscroll_active && autoscroll_toggle_mode {
                                            // Already in toggle mode - exit on middle click
                                            info!("AUTOSCROLL: Middle click in toggle mode - exiting");
                                            autoscroll_active = false;
                                            autoscroll_toggle_mode = false;
                                            middle_press_time = None;
                                            
                                            // Hide overlay indicator
                                            if let Some(ref sender) = overlay_sender {
                                                let _ = sender.send(OverlayCommand::Hide);
                                            }
                                            continue;
                                        } else {
                                            // Check if cursor is over a scrollable area
                                            let is_scrollable = if let Some(ref detector) = scroll_detector {
                                                detector.should_autoscroll()
                                            } else {
                                                // No detector available, default to allowing autoscroll everywhere
                                                true
                                            };
                                            
                                            if !is_scrollable {
                                                // NOT scrollable - emit a normal middle click and pass through
                                                debug!("AUTOSCROLL: Area not scrollable - passing through middle click");
                                                middle_passthrough = true;
                                                let press = InputEvent::new(EventType::KEY, BTN_MIDDLE, 1);
                                                let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                                                if let Err(e) = vdev.emit(&[press, sync]) {
                                                    warn!("Failed to emit middle press: {}", e);
                                                }
                                                // Don't enter autoscroll mode, continue to let release pass through
                                                continue;
                                            }
                                            
                                            // Start autoscroll (mode determined on release)
                                            info!("AUTOSCROLL: Middle button pressed - entering scroll mode");
                                            autoscroll_active = true;
                                            autoscroll_toggle_mode = false;  // Start as hold mode
                                            autoscroll_moved = false;
                                            middle_passthrough = false;
                                            middle_press_time = Some(Instant::now());
                                            scroll_tick_counter = 0;
                                            // Show overlay indicator at cursor position
                                            // Get fresh position from KWin (accurate on Wayland)
                                            // Fall back to tracked position if KWin fails
                                            if let Some(ref sender) = overlay_sender {
                                                let (show_x, show_y) = if let Some((kx, ky)) = get_cursor_position_kwin() {
                                                    info!("AUTOSCROLL: Got fresh KWin position ({}, {})", kx, ky);
                                                    (kx, ky)
                                                } else {
                                                    info!("AUTOSCROLL: KWin failed, using tracked position ({}, {})", abs_cursor_x, abs_cursor_y);
                                                    (abs_cursor_x, abs_cursor_y)
                                                };
                                                info!("AUTOSCROLL: Sending overlay Show at ({}, {})", show_x, show_y);
                                                let _ = sender.send(OverlayCommand::Show(show_x, show_y));
                                            }
                                            
                                            // Reset anchor AFTER KWin query completes to avoid twitch from
                                            // mouse movement that accumulated during the ~175ms KWin delay
                                            anchor_x = 0;
                                            anchor_y = 0;
                                            cursor_x = 0;
                                            cursor_y = 0;
                                            
                                            // Record activation time - we'll ignore scroll input for a grace period
                                            // to prevent any residual movement from causing initial twitch
                                            autoscroll_start_time = Some(Instant::now());
                                            
                                            // Don't pass through the middle button press
                                            continue;
                                        }
                                    } else if ev.value() == 0 {
                                        // Middle button released
                                        // First check if we're in passthrough mode (non-scrollable area)
                                        if middle_passthrough {
                                            debug!("AUTOSCROLL: Passing through middle release (non-scrollable area)");
                                            middle_passthrough = false;
                                            let release = InputEvent::new(EventType::KEY, BTN_MIDDLE, 0);
                                            let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                                            if let Err(e) = vdev.emit(&[release, sync]) {
                                                warn!("Failed to emit middle release: {}", e);
                                            }
                                            continue;
                                        }
                                        
                                        if autoscroll_active && !autoscroll_toggle_mode {
                                            // Determine click duration for behavior:
                                            // - Quick click (<150ms, no movement): pass through as normal click (for links etc)
                                            // - Medium hold (150-400ms, no movement): enter toggle autoscroll mode
                                            // - Long hold or moved: exit autoscroll (hold mode complete)
                                            let hold_duration = middle_press_time
                                                .map(|t| t.elapsed().as_millis() as u64)
                                                .unwrap_or(0);
                                            
                                            let was_quick_click = hold_duration < QUICK_CLICK_THRESHOLD_MS;
                                            let was_toggle_hold = hold_duration >= QUICK_CLICK_THRESHOLD_MS 
                                                && hold_duration < TOGGLE_MODE_THRESHOLD_MS;
                                            
                                            if was_quick_click && !autoscroll_moved {
                                                // Quick click with no movement - pass through as normal middle click
                                                // This allows clicking links, paste operations, etc.
                                                info!("AUTOSCROLL: Quick click - passing through for link/paste");
                                                autoscroll_active = false;
                                                middle_press_time = None;
                                                
                                                // Hide overlay indicator
                                                if let Some(ref sender) = overlay_sender {
                                                    let _ = sender.send(OverlayCommand::Hide);
                                                }
                                                
                                                // Emit normal middle click
                                                let press = InputEvent::new(EventType::KEY, BTN_MIDDLE, 1);
                                                let release = InputEvent::new(EventType::KEY, BTN_MIDDLE, 0);
                                                let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                                                if let Err(e) = vdev.emit(&[press, sync.clone(), release, sync]) {
                                                    warn!("Failed to emit middle click: {}", e);
                                                }
                                                continue;
                                            } else if was_toggle_hold && !autoscroll_moved {
                                                // Medium hold with no movement - enter toggle mode
                                                info!("AUTOSCROLL: Medium hold - entering toggle mode");
                                                autoscroll_toggle_mode = true;
                                                // Stay active, don't hide overlay
                                                continue;
                                            } else {
                                                // Long hold or moved - exit autoscroll (hold mode complete)
                                                info!("AUTOSCROLL: Hold mode release - exiting (moved={})", autoscroll_moved);
                                                autoscroll_active = false;
                                                middle_press_time = None;
                                                
                                                // Hide overlay indicator
                                                if let Some(ref sender) = overlay_sender {
                                                    let _ = sender.send(OverlayCommand::Hide);
                                                }
                                                continue;
                                            }
                                        }
                                        // In toggle mode, middle release is ignored (already handled on press)
                                        if autoscroll_toggle_mode {
                                            continue;
                                        }
                                    }
                                }
                                
                                // Any other button click while in autoscroll mode exits it
                                if autoscroll_active && ev.value() == 1 {
                                    info!("AUTOSCROLL: Other button pressed - exiting scroll mode");
                                    autoscroll_active = false;
                                    autoscroll_toggle_mode = false;
                                    middle_press_time = None;
                                    
                                    // Hide overlay indicator
                                    if let Some(ref sender) = overlay_sender {
                                        let _ = sender.send(OverlayCommand::Hide);
                                    }
                                    
                                    // Pass through this button press
                                }
                            }
                            
                            // Handle mouse movement in autoscroll mode
                            // Windows-style: cursor moves freely, scroll based on distance from anchor
                            if autoscroll_active {
                                if let InputEventKind::RelAxis(axis) = ev.kind() {
                                    match axis {
                                        evdev::RelativeAxisType::REL_X => {
                                            cursor_x += ev.value();
                                            abs_cursor_x = (abs_cursor_x + ev.value()).clamp(0, SCREEN_WIDTH);
                                            autoscroll_moved = true;
                                            // Pass through mouse movement so cursor moves
                                        }
                                        evdev::RelativeAxisType::REL_Y => {
                                            cursor_y += ev.value();
                                            abs_cursor_y = (abs_cursor_y + ev.value()).clamp(0, SCREEN_HEIGHT);
                                            autoscroll_moved = true;
                                            // Pass through mouse movement so cursor moves
                                        }
                                        _ => {}
                                    }
                                    
                                    scroll_tick_counter += 1;
                                    
                                    // Check if we should process scroll yet (grace period prevents immediate scrolling)
                                    let in_grace_period = autoscroll_start_time
                                        .map(|t| (t.elapsed().as_millis() as u64) < SCROLL_GRACE_PERIOD_MS)
                                        .unwrap_or(false);
                                    
                                    // During grace period, just pass through movement events normally
                                    // Only start scroll detection after grace period ends
                                    if !in_grace_period && scroll_tick_counter >= SCROLL_TICK_INTERVAL {
                                        scroll_tick_counter = 0;
                                        
                                        let dx = cursor_x - anchor_x;
                                        let dy = cursor_y - anchor_y;
                                        
                                        // Calculate scroll speed based on distance zones (gradual increase)
                                        let h_speed = calculate_scroll_speed(dx, SCROLL_DEAD_ZONE);
                                        let v_speed = calculate_scroll_speed(dy, SCROLL_DEAD_ZONE);
                                        
                                        // Horizontal scroll
                                        if h_speed > 0 {
                                            let scroll_val = if dx > 0 { h_speed } else { -h_speed };
                                            let scroll_ev = InputEvent::new(EventType::RELATIVE, REL_HWHEEL, scroll_val);
                                            let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                                            if let Err(e) = vdev.emit(&[scroll_ev, sync]) {
                                                warn!("Failed to emit hwheel: {}", e);
                                            }
                                        }
                                        
                                        // Vertical scroll
                                        if v_speed > 0 {
                                            // Negative because mouse down = scroll down (content up)
                                            let scroll_val = if dy > 0 { -v_speed } else { v_speed };
                                            let scroll_ev = InputEvent::new(EventType::RELATIVE, REL_WHEEL, scroll_val);
                                            let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                                            if let Err(e) = vdev.emit(&[scroll_ev, sync]) {
                                                warn!("Failed to emit wheel: {}", e);
                                            }
                                        }
                                        
                                        // Update overlay direction (throttled)
                                        if scroll_tick_counter % DIRECTION_UPDATE_INTERVAL == 0 {
                                            if let Some(ref sender) = overlay_sender {
                                                let norm_dx = (dx as f32 / 100.0).clamp(-1.0, 1.0);
                                                let norm_dy = (dy as f32 / 100.0).clamp(-1.0, 1.0);
                                                let _ = sender.send(OverlayCommand::UpdateDirection(norm_dx, norm_dy));
                                            }
                                        }
                                    }  // End of if/else block for grace period and scroll_tick check
                                    
                                    // DON'T continue here - let mouse movement pass through
                                }
                            }
                        }
                        
                        // Debug log events by type
                        match ev.kind() {
                            InputEventKind::Key(key) => {
                                info!("KEY event: code={}, value={}", key.code(), ev.value());
                            }
                            InputEventKind::RelAxis(_axis) => {
                                // Cursor position is tracked at the start of event loop
                                // Uncomment for debugging: info!("REL event: axis={:?}, value={}", axis, ev.value());
                            }
                            InputEventKind::Synchronization(_) => {
                                // Don't log sync events (too noisy)
                            }
                            _ => {
                                info!("OTHER event: type={:?}, code={}, value={}", ev.event_type(), ev.code(), ev.value());
                            }
                        }
                        if let Some(mapped_events) = remap_events(&config.mappings, ev, &macros) {
                            if let Err(e) = vdev.emit(&mapped_events) {
                                warn!("uinput emit failed: {e}");
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No events available, continue to next device
                }
                Err(e) => return Err(e).context("Failed to read events from evdev device"),
            }
        }
        
        if !had_events {
            thread::sleep(Duration::from_millis(5));
        }
    }

    // Best-effort ungrab. (Dropping the devices should also release the grabs.)
    for mut dev in devices {
        let _ = dev.ungrab();
    }
    Ok(())
}

fn remap_events(
    mappings: &BTreeMap<u16, MappingTarget>,
    ev: InputEvent,
    macros: &std::collections::HashMap<u32, crate::profile::Macro>,
) -> Option<Vec<InputEvent>> {
    // Special codes for scroll wheel emulation
    const SCROLL_UP_CODE: u16 = 280;
    const SCROLL_DOWN_CODE: u16 = 281;
    // Macro target codes are 1000+ (1001 = macro id 1, etc.)
    const MACRO_CODE_BASE: u16 = 1000;
    // REL_WHEEL axis code
    const REL_WHEEL: u16 = 8;
    
    match ev.kind() {
        InputEventKind::Key(key) => {
            let src_code: u16 = key.code();
            let value = ev.value();
            if let Some(target) = mappings.get(&src_code) {
                info!("REMAP: code {} -> {} (value={})", src_code, target.base, value);
                let mut out: Vec<InputEvent> = Vec::new();

                // Handle special scroll wheel emulation codes
                if target.base == SCROLL_UP_CODE || target.base == SCROLL_DOWN_CODE {
                    // Only emit scroll on key press (value=1), not release or repeat
                    if value == 1 {
                        let scroll_value = if target.base == SCROLL_UP_CODE { 1 } else { -1 };
                        info!("SCROLL: emitting REL_WHEEL value={}", scroll_value);
                        out.push(InputEvent::new(EventType::RELATIVE, REL_WHEEL, scroll_value));
                    }
                    return Some(out);
                }
                
                // Handle macro target codes
                if target.base > MACRO_CODE_BASE && target.base < 2000 {
                    let macro_id = (target.base - MACRO_CODE_BASE) as u32;
                    // Only trigger on key press (value=1), not release
                    if value == 1 {
                        info!("MACRO: triggering macro id={}", macro_id);
                        if let Some(macro_data) = macros.get(&macro_id) {
                            // Execute macro in a background thread to avoid blocking input
                            let macro_clone = macro_data.clone();
                            std::thread::spawn(move || {
                                if let Err(e) = crate::macro_engine::execute_macro(&macro_clone) {
                                    warn!("Macro execution failed: {}", e);
                                }
                            });
                        } else {
                            warn!("Macro id={} not found in remapper's macro cache", macro_id);
                        }
                    }
                    // Don't emit any key events for macros
                    return Some(vec![]);
                }

                match value {
                    1 => {
                        // Press: press modifiers first, then base key
                        for m in target.mods.to_key_codes() {
                            out.push(InputEvent::new(EventType::KEY, m, 1));
                        }
                        out.push(InputEvent::new(EventType::KEY, target.base, 1));
                    }
                    0 => {
                        // Release: release base, then modifiers
                        out.push(InputEvent::new(EventType::KEY, target.base, 0));
                        for m in target.mods.to_key_codes() {
                            out.push(InputEvent::new(EventType::KEY, m, 0));
                        }
                    }
                    2 => {
                        // Repeat: repeat base only
                        out.push(InputEvent::new(EventType::KEY, target.base, 2));
                    }
                    _ => {
                        out.push(ev);
                    }
                }

                Some(out)
            } else {
                Some(vec![ev])
            }
        }
        _ => Some(vec![ev]),
    }
}

pub fn capture_next_key_code(timeout: Duration, preferred_device: Option<&str>) -> Result<u16> {
    // Collect all unique candidate paths, explicitly including ALL Razer interfaces
    // The Naga Trinity exposes multiple interfaces:
    //   - input0 (event8): Mouse interface with 5 buttons (left/right/middle/side/extra)
    //   - input1 (event9): "Keyboard" interface - receives side panel keys as keyboard codes
    //   - input1 (event10): Absolute axis interface (no keys)
    //   - input2 (event11): Another keyboard interface - may also receive side panel keys
    // We need to listen to ALL of them to capture side button presses.
    let mut paths: Vec<PathBuf> = Vec::new();

    info!("Scanning for ALL Razer input interfaces...");

    // Include ALL Razer/Naga interfaces that have any key capabilities
    for (path, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or_default().to_string();
        let name_lower = name.to_ascii_lowercase();
        let is_razer = name_lower.contains("razer") || name_lower.contains("naga");
        
        if !is_razer {
            continue;
        }

        // Check if this interface has any key capabilities at all
        let has_keys = dev.supported_keys()
            .map(|k| k.iter().next().is_some())
            .unwrap_or(false);
        
        if has_keys {
            info!("  Found Razer interface with keys: {:?} ({})", path, name);
            paths.push(path);
        } else {
            info!("  Skipping Razer interface (no keys): {:?} ({})", path, name);
        }
    }

    // Fallback: if no Razer devices found, try the heuristic selection
    if paths.is_empty() {
        if let Some(p) = select_source_device(&preferred_device.map(|s| s.to_string())) {
            paths.push(p);
        }
    }

    // Deduplicate
    paths.sort();
    paths.dedup();

    if paths.is_empty() {
        anyhow::bail!("No suitable /dev/input/event* device found");
    }

    info!("Learn: listening simultaneously on {} devices: {:?}", paths.len(), paths);

    // Open all devices
    let mut devices: Vec<(Device, String)> = Vec::new();
    for path in &paths {
        info!("Attempting to open device: {:?}", path);
        match Device::open(path) {
            Ok(dev) => {
                 let name = dev.name().unwrap_or("?").to_string();
                 info!("Successfully opened: {:?} ({})", path, name);
                 
                 // Set non-blocking to allow polling multiple devices
                 if let Err(e) = set_nonblocking(&dev) {
                     warn!("Failed to set non-blocking on {:?}: {}", path, e);
                     continue;
                 }
                 devices.push((dev, name));
            }
            Err(e) => {
                warn!("Failed to open candidate device {:?}: {} ({})", path, e, e.kind());
            }
        }
    }

    if devices.is_empty() {
        anyhow::bail!("Failed to open any candidate devices (check permissions?).");
    }

    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let mut any_events_this_loop = false;

        for (dev, name) in &mut devices {
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        any_events_this_loop = true;
                        // Log events for debugging visibility
                        if let InputEventKind::Key(key) = ev.kind() {
                            info!("Key event on {}: code={} (0x{:04x}) val={}",
                                  name, key.code(), key.code(), ev.value());

                            if ev.value() == 1 { // Press
                                info!("Captured key code: {} (0x{:04x}) from {}", key.code(), key.code(), name);
                                return Ok(key.code());
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No events on this device
                }
                Err(e) => {
                    warn!("Error reading from device {}: {}", name, e);
                }
            }
        }

        if !any_events_this_loop {
            thread::sleep(Duration::from_millis(10));
        }
    }

    anyhow::bail!("Timed out waiting for button press.");
}

// ========== Persistent Key Capture for Macro Recording ==========

use std::sync::mpsc;

/// A captured keyboard event for macro recording
#[derive(Debug, Clone)]
pub struct CapturedKey {
    pub code: u16,
    pub is_press: bool,
}

/// Persistent key listener that captures keyboard events during macro recording.
/// This is much more reliable than per-key capture because:
/// 1. Devices are opened once and kept open
/// 2. All key presses AND releases are captured automatically
/// 3. No re-scanning between each key
pub struct KeyCaptureListener {
    stop_flag: Arc<AtomicBool>,
    receiver: mpsc::Receiver<CapturedKey>,
    _thread: std::thread::JoinHandle<()>,
}

impl KeyCaptureListener {
    /// Start a persistent key capture listener.
    /// Returns immediately with a listener that receives key events.
    pub fn start() -> Result<Self> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        
        let (sender, receiver) = mpsc::channel::<CapturedKey>();
        
        // Try to open keyboard devices before spawning thread
        let mut paths: Vec<PathBuf> = Vec::new();
        
        info!("KeyCaptureListener: Scanning for keyboard devices...");
        
        for (path, dev) in evdev::enumerate() {
            let name = dev.name().unwrap_or_default().to_string();
            
            // Check if this is a keyboard (has regular keyboard keys)
            let has_keyboard = dev.supported_keys()
                .map(|k| {
                    k.contains(Key::KEY_A) || k.contains(Key::KEY_1) || k.contains(Key::KEY_SPACE)
                })
                .unwrap_or(false);
            
            if has_keyboard {
                info!("  Found keyboard: {:?} ({})", path, name);
                paths.push(path);
            }
        }
        
        if paths.is_empty() {
            anyhow::bail!("No keyboard devices found");
        }
        
        // Open devices
        let mut devices: Vec<(Device, String)> = Vec::new();
        for path in &paths {
            match Device::open(path) {
                Ok(dev) => {
                    let name = dev.name().unwrap_or("?").to_string();
                    if let Err(e) = set_nonblocking(&dev) {
                        warn!("Failed to set non-blocking on {:?}: {}", path, e);
                        continue;
                    }
                    devices.push((dev, name));
                }
                Err(e) => {
                    warn!("Failed to open keyboard {:?}: {}", path, e);
                }
            }
        }
        
        if devices.is_empty() {
            anyhow::bail!("Permission denied: Add user to 'input' group with: sudo usermod -aG input $USER (then log out/in)");
        }
        
        info!("KeyCaptureListener: Started listening on {} keyboard(s)", devices.len());
        
        let thread = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                for (dev, _name) in &mut devices {
                    match dev.fetch_events() {
                        Ok(events) => {
                            for ev in events {
                                if let InputEventKind::Key(key) = ev.kind() {
                                    // value=1 is press, value=0 is release
                                    if ev.value() == 1 || ev.value() == 0 {
                                        let captured = CapturedKey {
                                            code: key.code(),
                                            is_press: ev.value() == 1,
                                        };
                                        if sender.send(captured).is_err() {
                                            // Receiver dropped, stop listening
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => {}
                    }
                }
                thread::sleep(Duration::from_millis(5));
            }
        });
        
        Ok(Self {
            stop_flag,
            receiver,
            _thread: thread,
        })
    }
    
    /// Try to receive a captured key (non-blocking)
    pub fn try_recv(&self) -> Option<CapturedKey> {
        self.receiver.try_recv().ok()
    }
    
    /// Wait for the next key with timeout
    pub fn recv_timeout(&self, timeout: Duration) -> Option<CapturedKey> {
        self.receiver.recv_timeout(timeout).ok()
    }
    
    /// Stop the listener
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

impl Drop for KeyCaptureListener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Capture a single keypress for macro recording
/// Returns (key_code, is_press) - captures both press and release events
pub fn capture_key_for_macro(timeout: Duration) -> Result<(u16, bool)> {
    // Find keyboard devices (not just Razer - any keyboard will do for macro recording)
    let mut paths: Vec<PathBuf> = Vec::new();

    info!("Scanning for keyboard devices for macro key capture...");

    for (path, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or_default().to_string();
        
        // Check if this is a keyboard (has regular keyboard keys)
        let has_keyboard = dev.supported_keys()
            .map(|k| {
                k.contains(Key::KEY_A) || k.contains(Key::KEY_1) || k.contains(Key::KEY_SPACE)
            })
            .unwrap_or(false);
        
        if has_keyboard {
            info!("  Found keyboard: {:?} ({})", path, name);
            paths.push(path);
        }
    }

    if paths.is_empty() {
        anyhow::bail!("No keyboard devices found for macro recording");
    }

    info!("Macro capture: listening on {} keyboard(s)", paths.len());

    // Open devices
    let mut devices: Vec<(Device, String)> = Vec::new();
    for path in &paths {
        match Device::open(path) {
            Ok(dev) => {
                let name = dev.name().unwrap_or("?").to_string();
                if let Err(e) = set_nonblocking(&dev) {
                    warn!("Failed to set non-blocking on {:?}: {}", path, e);
                    continue;
                }
                devices.push((dev, name));
            }
            Err(e) => {
                warn!("Failed to open keyboard {:?}: {}", path, e);
            }
        }
    }

    if devices.is_empty() {
        anyhow::bail!("Permission denied: Add user to 'input' group with: sudo usermod -aG input $USER (then log out/in) OR run with: sudo -E razerlinux");
    }

    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let mut any_events = false;

        for (dev, name) in &mut devices {
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        any_events = true;
                        if let InputEventKind::Key(key) = ev.kind() {
                            // Capture key press (value=1) or release (value=0)
                            if ev.value() == 1 || ev.value() == 0 {
                                let is_press = ev.value() == 1;
                                info!("Macro: Captured key {} {} from {}", 
                                      key.code(), 
                                      if is_press { "PRESS" } else { "RELEASE" },
                                      name);
                                return Ok((key.code(), is_press));
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => warn!("Error reading from {}: {}", name, e),
            }
        }

        if !any_events {
            thread::sleep(Duration::from_millis(10));
        }
    }

    anyhow::bail!("Timed out waiting for key press");
}

/// Select ALL Razer keyboard interfaces for grabbing.
/// The Naga Trinity sends side button keys through multiple interfaces (event9 AND event11),
/// so we need to grab all of them to properly intercept the keys.
fn select_all_razer_keyboard_devices(preferred_device: &Option<String>) -> Vec<PathBuf> {
    // If a preferred device is specified, only use that one
    if let Some(p) = preferred_device {
        let path = PathBuf::from(p);
        if path.exists() {
            return vec![path];
        }
    }

    let mut razer_devices: Vec<PathBuf> = Vec::new();
    
    info!("Scanning for ALL Razer interfaces to grab (keyboard + mouse + DPI)...");
    
    for (path, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or_default().to_string();
        let name_lower = name.to_ascii_lowercase();

        let is_razer = name_lower.contains("razer") || name_lower.contains("naga");
        let is_dpi_device = name.contains("RazerLinux DPI");
        
        if !is_razer && !is_dpi_device {
            continue;
        }
        
        let keys = dev.supported_keys();
        
        // Check for keyboard keys (side buttons in Driver Mode)
        let num_keys = keys.as_ref().map(|k| {
            k.iter().filter(|key| key.code() < 0x100).count()
        }).unwrap_or(0);
        
        // Check for mouse buttons (thumb buttons: BTN_SIDE, BTN_EXTRA, etc.)
        let has_thumb_btns = keys.as_ref().map(|k| {
            k.contains(Key::BTN_SIDE) || k.contains(Key::BTN_EXTRA) ||
            k.contains(Key::BTN_FORWARD) || k.contains(Key::BTN_BACK)
        }).unwrap_or(false);
        
        // Check for main mouse buttons (BTN_LEFT, BTN_RIGHT, BTN_MIDDLE)
        // Needed for autoscroll which intercepts BTN_MIDDLE
        let has_main_mouse_btns = keys.as_ref().map(|k| {
            k.contains(Key::BTN_LEFT) || k.contains(Key::BTN_MIDDLE)
        }).unwrap_or(false);
        
        // Grab the DPI button virtual device (it has F13/F14 keys)
        if is_dpi_device {
            info!("  Found DPI button virtual device: {:?}", path);
            razer_devices.push(path);
        }
        // Grab interfaces that have keyboard keys OR mouse thumb buttons OR main mouse buttons
        else if num_keys > 0 {
            info!("  Found Razer keyboard interface: {:?} [keys={}]", path, num_keys);
            razer_devices.push(path);
        } else if has_thumb_btns {
            info!("  Found Razer mouse interface: {:?} [has_thumb_btns=true]", path);
            razer_devices.push(path);
        } else if has_main_mouse_btns {
            info!("  Found Razer main mouse interface: {:?} [has_main_btns=true]", path);
            razer_devices.push(path);
        }
    }
    
    if razer_devices.is_empty() {
        // Fall back to single device selection if no interfaces found
        if let Some(p) = select_source_device(&None) {
            return vec![p];
        }
    }
    
    razer_devices
}

fn select_source_device(preferred_device: &Option<String>) -> Option<PathBuf> {
    if let Some(p) = preferred_device {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    // Heuristic tiers (first match wins), but we also track the richest Razer interface:
    // 1) Razer/Naga device exposing mouse buttons
    // 2) Any device exposing mouse buttons
    // 3) Razer/Naga device with relative axes
    // 4) Any device with relative axes
    // 5) Razer/Naga device with keys
    // 6) Any device with keys
    // Additionally, we track the Razer interface with the highest (buttons, keys) count, so
    // side-button keyboard interfaces win over the plain mouse interface.
    let mut tier2: Option<PathBuf> = None;
    let mut tier4: Option<PathBuf> = None;
    let mut tier6: Option<PathBuf> = None;
    let mut best_razer: Option<(PathBuf, usize, usize)> = None;

    info!("Scanning /dev/input/event* devices for mouse input...");
    
    // Also check sibling interfaces for multi-interface devices (like Razer Naga with kbd interfaces)
    let mut all_razer_devices: Vec<(PathBuf, String, usize, usize)> = Vec::new();
    
    for (path, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or_default().to_string();
        let name_lower = name.to_ascii_lowercase();

        let keys = dev.supported_keys();
        let has_mouse_btns = keys
            .as_ref()
            .map(|k| {
                k.contains(Key::BTN_LEFT)
                    || k.contains(Key::BTN_RIGHT)
                    || k.contains(Key::BTN_MIDDLE)
                    || k.contains(Key::BTN_SIDE)
                    || k.contains(Key::BTN_EXTRA)
                    || k.contains(Key::BTN_FORWARD)
                    || k.contains(Key::BTN_BACK)
                    || k.contains(Key::BTN_TASK)
            })
            .unwrap_or(false);
            
        // Count actual button and key types for debugging
        let num_buttons = keys.as_ref().map(|k| {
            k.iter().filter(|key| key.code() >= 0x110 && key.code() < 0x160).count()
        }).unwrap_or(0);
        
        let num_keys = keys.as_ref().map(|k| {
            k.iter().filter(|key| key.code() < 0x100).count()
        }).unwrap_or(0);

        let has_rel = dev
            .supported_relative_axes()
            .map(|r| r.iter().next().is_some())
            .unwrap_or(false);
        let has_keys = keys
            .as_ref()
            .map(|k| k.iter().next().is_some())
            .unwrap_or(false);

        let is_razer = name_lower.contains("razer") || name_lower.contains("naga");

        info!("  {:?}: '{}' mouse_btns={} rel={} keys={} razer={} [btns={} keys={}]",
              path.display(), name, has_mouse_btns, has_rel, has_keys, is_razer, num_buttons, num_keys);
              
        // Track all Razer devices and remember the richest one
        if is_razer {
            all_razer_devices.push((path.clone(), name.clone(), num_buttons, num_keys));

            // Score by total capabilities; side-button keyboard interface should win over plain mouse interface
            let score = num_buttons as u32 + num_keys as u32;
            let better = match best_razer {
                None => true,
                Some((_, b_btns, b_keys)) => score > b_btns as u32 + b_keys as u32,
            };
            if better {
                best_razer = Some((path.clone(), num_buttons, num_keys));
            }
        }

        if has_mouse_btns {
            if !is_razer && tier2.is_none() {
                tier2 = Some(path.clone());
            }
            continue;
        }

        if has_rel {
            if !is_razer && tier4.is_none() {
                tier4 = Some(path.clone());
            }
            continue;
        }

        if has_keys {
            if !is_razer && tier6.is_none() {
                tier6 = Some(path.clone());
            }
        }
    }

    // Prefer the richest Razer interface (many buttons/keys) before heuristics
    if let Some((best_path, btns, keys)) = best_razer {
        info!(
            "Selected richest Razer interface: {:?} [btns={} keys={}]",
            best_path, btns, keys
        );
        return Some(best_path);
    }

    if let Some(ref p) = tier2 {
        info!("Selected fallback (tier 2: any device with mouse buttons): {:?}", p);
        return tier2;
    }
    if let Some(ref p) = tier4 {
        info!("Selected fallback (tier 4: any device with relative axes): {:?}", p);
        return tier4;
    }
    if let Some(ref p) = tier6 {
        info!("Selected fallback (tier 6: any device with keys): {:?}", p);
        return tier6;
    }

    warn!("No suitable input device found!");
    None
}

fn set_nonblocking(dev: &Device) -> Result<()> {
    let raw_fd = dev.as_raw_fd();

    // Preserve existing flags; just OR in O_NONBLOCK.
    let current = unsafe { libc::fcntl(raw_fd, libc::F_GETFL) };
    if current < 0 {
        return Err(std::io::Error::last_os_error()).context("fcntl(F_GETFL) failed");
    }

    let rc = unsafe { libc::fcntl(raw_fd, libc::F_SETFL, current | libc::O_NONBLOCK) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error()).context("fcntl(F_SETFL, O_NONBLOCK) failed");
    }
    Ok(())
}
