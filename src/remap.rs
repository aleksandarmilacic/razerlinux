//! Software button remapping (evdev grab + uinput virtual device)

use crate::overlay::OverlayCommand;
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
use tracing::{info, warn};

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
    pub fn start(config: RemapConfig, overlay_sender: Option<Sender<OverlayCommand>>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();

        let ext_config = RemapConfigExt {
            config,
            overlay_sender,
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
    
    // Find ALL Razer keyboard interfaces - the Naga Trinity sends side button keys
    // through multiple interfaces (event9 AND event11), so we need to grab them all
    let source_paths = select_all_razer_keyboard_devices(&config.source_device);
    
    if source_paths.is_empty() {
        anyhow::bail!("No suitable Razer keyboard interfaces found for remapping");
    }

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

    // Autoscroll state - Windows style: cursor moves, distance from anchor controls scroll
    let mut autoscroll_active = false;
    let mut autoscroll_moved = false;  // Track if mouse moved during autoscroll
    let mut anchor_x: i32 = 0;  // Anchor point X (where middle-click happened)
    let mut anchor_y: i32 = 0;  // Anchor point Y
    let mut cursor_x: i32 = 0;  // Current cursor position X
    let mut cursor_y: i32 = 0;  // Current cursor position Y
    let mut scroll_tick_counter: u32 = 0;  // For throttling scroll events
    const SCROLL_THRESHOLD: i32 = 20;  // Pixels from anchor before scrolling starts
    const SCROLL_SPEED_DIVISOR: f32 = 50.0;  // Higher = slower scrolling
    const SCROLL_TICK_INTERVAL: u32 = 5;  // Emit scroll every N movement events
    const DIRECTION_UPDATE_INTERVAL: u32 = 20;  // Update overlay direction every N events
    const BTN_MIDDLE: u16 = 274;
    const REL_WHEEL: u16 = 8;
    const REL_HWHEEL: u16 = 6;

    while !stop.load(Ordering::Relaxed) {
        let mut had_events = false;
        
        for dev in &mut devices {
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        had_events = true;
                        
                        // Handle autoscroll if enabled
                        if config.autoscroll_enabled {
                            // Check for middle button press/release
                            if let InputEventKind::Key(key) = ev.kind() {
                                if key.code() == BTN_MIDDLE {
                                    if ev.value() == 1 {
                                        // Middle button pressed - enter autoscroll mode
                                        info!("AUTOSCROLL: Middle button pressed - entering scroll mode");
                                        autoscroll_active = true;
                                        autoscroll_moved = false;
                                        scroll_tick_counter = 0;
                                        // Set anchor to current cursor position (we'll track relative movement)
                                        anchor_x = 0;
                                        anchor_y = 0;
                                        cursor_x = 0;
                                        cursor_y = 0;
                                        
                                        // Show overlay indicator
                                        if let Some(ref sender) = overlay_sender {
                                            let _ = sender.send(OverlayCommand::Show);
                                        }
                                        
                                        // Don't pass through the middle button press
                                        continue;
                                    } else if ev.value() == 0 {
                                        // Middle button released
                                        if autoscroll_active {
                                            info!("AUTOSCROLL: Middle button released - exiting scroll mode (moved={})", autoscroll_moved);
                                            autoscroll_active = false;
                                            
                                            // Hide overlay indicator
                                            if let Some(ref sender) = overlay_sender {
                                                let _ = sender.send(OverlayCommand::Hide);
                                            }
                                            
                                            // If we didn't move, emit a normal middle click
                                            if !autoscroll_moved {
                                                info!("AUTOSCROLL: No movement - emitting normal middle click");
                                                let press = InputEvent::new(EventType::KEY, BTN_MIDDLE, 1);
                                                let release = InputEvent::new(EventType::KEY, BTN_MIDDLE, 0);
                                                let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                                                if let Err(e) = vdev.emit(&[press, sync.clone(), release, sync]) {
                                                    warn!("Failed to emit middle click: {}", e);
                                                }
                                            }
                                            // Don't pass through the middle button release
                                            continue;
                                        }
                                    }
                                }
                                
                                // Any other button click while in autoscroll mode exits it
                                if autoscroll_active && ev.value() == 1 {
                                    info!("AUTOSCROLL: Other button pressed - exiting scroll mode");
                                    autoscroll_active = false;
                                    
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
                                            autoscroll_moved = true;
                                            // Pass through mouse movement so cursor moves
                                        }
                                        evdev::RelativeAxisType::REL_Y => {
                                            cursor_y += ev.value();
                                            autoscroll_moved = true;
                                            // Pass through mouse movement so cursor moves
                                        }
                                        _ => {}
                                    }
                                    
                                    scroll_tick_counter += 1;
                                    
                                    // Emit scroll events periodically based on distance from anchor
                                    if scroll_tick_counter >= SCROLL_TICK_INTERVAL {
                                        scroll_tick_counter = 0;
                                        
                                        let dx = cursor_x - anchor_x;
                                        let dy = cursor_y - anchor_y;
                                        
                                        // Only scroll if beyond threshold
                                        if dx.abs() > SCROLL_THRESHOLD {
                                            let scroll_amount = ((dx.abs() - SCROLL_THRESHOLD) as f32 / SCROLL_SPEED_DIVISOR) as i32;
                                            if scroll_amount > 0 {
                                                let scroll_val = if dx > 0 { scroll_amount } else { -scroll_amount };
                                                let scroll_ev = InputEvent::new(EventType::RELATIVE, REL_HWHEEL, scroll_val);
                                                let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                                                if let Err(e) = vdev.emit(&[scroll_ev, sync]) {
                                                    warn!("Failed to emit hwheel: {}", e);
                                                }
                                            }
                                        }
                                        
                                        if dy.abs() > SCROLL_THRESHOLD {
                                            let scroll_amount = ((dy.abs() - SCROLL_THRESHOLD) as f32 / SCROLL_SPEED_DIVISOR) as i32;
                                            if scroll_amount > 0 {
                                                // Negative because mouse down = scroll down (content up)
                                                let scroll_val = if dy > 0 { -scroll_amount } else { scroll_amount };
                                                let scroll_ev = InputEvent::new(EventType::RELATIVE, REL_WHEEL, scroll_val);
                                                let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                                                if let Err(e) = vdev.emit(&[scroll_ev, sync]) {
                                                    warn!("Failed to emit wheel: {}", e);
                                                }
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
                                    }
                                    
                                    // DON'T continue here - let mouse movement pass through
                                }
                            }
                        }
                        
                        // Debug log events by type
                        match ev.kind() {
                            InputEventKind::Key(key) => {
                                info!("KEY event: code={}, value={}", key.code(), ev.value());
                            }
                            InputEventKind::RelAxis(axis) => {
                                info!("REL event: axis={:?}, value={}", axis, ev.value());
                            }
                            InputEventKind::Synchronization(_) => {
                                // Don't log sync events (too noisy)
                            }
                            _ => {
                                info!("OTHER event: type={:?}, code={}, value={}", ev.event_type(), ev.code(), ev.value());
                            }
                        }
                        if let Some(mapped_events) = remap_events(&config.mappings, ev) {
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
) -> Option<Vec<InputEvent>> {
    // Special codes for scroll wheel emulation
    const SCROLL_UP_CODE: u16 = 280;
    const SCROLL_DOWN_CODE: u16 = 281;
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
