//! RazerLinux - Razer Mouse Configuration Tool
//!
//! A userspace application for configuring Razer mice on Linux
//! without requiring kernel drivers.

mod device;
mod hidpoll;
mod profile;
mod protocol;
mod remap;

use anyhow::Result;
use profile::{Profile, ProfileManager};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;
use tracing::{error, info, warn};

slint::include_modules!();

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("RazerLinux starting...");

    // Log all detected Razer input interfaces for debugging
    let interfaces = remap::list_razer_input_interfaces();
    if interfaces.is_empty() {
        info!("No Razer input interfaces found in /dev/input/");
    } else {
        info!("Detected {} Razer input interface(s):", interfaces.len());
        for iface in &interfaces {
            info!(
                "  {:?}: '{}' [mouse_btns={}, kbd_keys={}, buttons={}, keys={}]",
                iface.path, iface.name, iface.has_mouse_buttons, iface.has_keyboard_keys,
                iface.num_buttons, iface.num_keys
            );
        }
    }

    // Create the main window
    let main_window = MainWindow::new()?;

    // Shared device state
    let device: Rc<RefCell<Option<device::RazerDevice>>> = Rc::new(RefCell::new(None));

    // Shared remapping state
    let remapper: Rc<RefCell<Option<remap::Remapper>>> = Rc::new(RefCell::new(None));
    let remap_mappings: Rc<RefCell<BTreeMap<u16, remap::MappingTarget>>> =
        Rc::new(RefCell::new(BTreeMap::new()));
    
    // DPI button poller - polls hidraw for DPI button presses and injects F13/F14 events
    let dpi_poller: Rc<RefCell<Option<hidpoll::DpiButtonPoller>>> = Rc::new(RefCell::new(None));

    // Try to find and connect to device on startup
    connect_device(&main_window, &device);

    // Setup callbacks
    setup_callbacks(&main_window, device, remapper, remap_mappings, dpi_poller);

    // Run the GUI event loop
    info!("Starting GUI...");
    main_window.run()?;

    info!("RazerLinux shutting down");
    Ok(())
}

fn connect_device(window: &MainWindow, device: &Rc<RefCell<Option<device::RazerDevice>>>) {
    match device::find_naga_trinity() {
        Ok(Some(device_info)) => {
            info!("Found Razer Naga Trinity at {}", device_info.path);

            match device::RazerDevice::open(&device_info.path) {
                Ok(mut dev) => {
                    info!("Device opened successfully!");

                    // Check and log device mode
                    match dev.get_device_mode() {
                        Ok((mode, param)) => {
                            info!("Device mode: {:#04x}, param: {:#04x}", mode, param);
                            // Mode 0x00 = Normal (hardware handles buttons)
                            // Mode 0x03 = Driver mode (software handles buttons - side buttons send keyboard keys)
                            if mode == 0x00 {
                                info!("Device is in Normal mode");
                            } else if mode == 0x03 {
                                info!("Device is in Driver mode - restoring Normal mode on startup");
                                // Ensure we're in Normal mode on startup for clean state
                                if let Err(e) = dev.disable_driver_mode() {
                                    warn!("Failed to restore Normal mode: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to get device mode: {} (this may be normal)", e);
                        }
                    }

                    // Update UI with device info
                    window.set_device_name(device_info.product.into());
                    window.set_device_connected(true);
                    window.set_status_message("Connected".into());

                    // Read firmware version
                    match dev.get_firmware_version() {
                        Ok(version) => window.set_firmware_version(version.into()),
                        Err(_) => window.set_firmware_version("-".into()),
                    }

                    // Read current DPI
                    match dev.get_dpi() {
                        Ok((dpi_x, dpi_y)) => {
                            info!("Current DPI: {}x{}", dpi_x, dpi_y);
                            window.set_current_dpi_x(dpi_x as i32);
                            window.set_current_dpi_y(dpi_y as i32);
                        }
                        Err(e) => {
                            warn!("Failed to read DPI: {}", e);
                        }
                    }

                    // Store device handle
                    *device.borrow_mut() = Some(dev);
                }
                Err(e) => {
                    error!("Failed to open device: {}", e);
                    window.set_status_message(format!("Error: {}", e).into());
                }
            }
        }
        Ok(None) => {
            info!("No Razer Naga Trinity found");
            window.set_device_name("No device found".into());
            window.set_device_connected(false);
            window.set_status_message("Plug in your Razer mouse".into());
        }
        Err(e) => {
            error!("Error scanning for devices: {}", e);
            window.set_status_message(format!("Scan error: {}", e).into());
        }
    }
}

fn setup_callbacks(
    window: &MainWindow,
    device: Rc<RefCell<Option<device::RazerDevice>>>,
    remapper: Rc<RefCell<Option<remap::Remapper>>>,
    remap_mappings: Rc<RefCell<BTreeMap<u16, remap::MappingTarget>>>,
    dpi_poller: Rc<RefCell<Option<hidpoll::DpiButtonPoller>>>,
) {
    // Apply DPI callback
    let device_clone = device.clone();
    let window_weak = window.as_weak();
    window.on_apply_dpi(move |dpi_x, dpi_y| {
        info!("Setting DPI to {}x{}", dpi_x, dpi_y);

        if let Some(ref mut dev) = *device_clone.borrow_mut() {
            match dev.set_dpi(dpi_x as u16, dpi_y as u16) {
                Ok(()) => {
                    info!("DPI set successfully!");
                    if let Some(win) = window_weak.upgrade() {
                        win.set_status_message("DPI applied!".into());
                    }
                }
                Err(e) => {
                    error!("Failed to set DPI: {}", e);
                    if let Some(win) = window_weak.upgrade() {
                        win.set_status_message(format!("Error: {}", e).into());
                    }
                }
            }
        }
    });

    // Refresh device callback
    let device_clone = device.clone();
    let window_weak = window.as_weak();
    window.on_refresh_device(move || {
        info!("Refreshing device connection...");
        if let Some(win) = window_weak.upgrade() {
            // Clear current device
            *device_clone.borrow_mut() = None;
            win.set_device_connected(false);
            win.set_status_message("Scanning...".into());

            // Try to reconnect
            connect_device_inner(&win, &device_clone);
        }
    });

    // Save profile callback
    let remap_mappings_clone = remap_mappings.clone();
    let remapper_clone = remapper.clone();
    let window_weak = window.as_weak();
    window.on_save_profile(move |profile_name| {
        info!("Saving profile: {}", profile_name);
        if let Some(win) = window_weak.upgrade() {
            let name = profile_name.to_string();
            if name.is_empty() {
                win.set_status_message("Enter a profile name first".into());
                return;
            }

            let dpi_x = win.get_current_dpi_x() as u16;
            let dpi_y = win.get_current_dpi_y() as u16;
            let mut profile = Profile::from_device_settings(&name, dpi_x, dpi_y);
            profile.remap.enabled = win.get_remap_enabled();
            profile.remap.mappings = remap_mappings_clone
                .borrow()
                .iter()
                .map(|(s, t)| profile::RemapMapping {
                    source: *s,
                    target: t.base,
                    ctrl: t.mods.ctrl,
                    alt: t.mods.alt,
                    shift: t.mods.shift,
                    meta: t.mods.meta,
                })
                .collect();

            // If remapping is currently active, store the detected/selected device if any.
            if profile.remap.enabled {
                profile.remap.source_device = None;
            } else {
                // If disabled, still keep existing source_device if user loaded a profile.
            }

            match ProfileManager::new() {
                Ok(manager) => match manager.save_profile(&profile) {
                    Ok(_) => win.set_status_message(format!("Profile '{}' saved!", name).into()),
                    Err(e) => win.set_status_message(format!("Save error: {}", e).into()),
                },
                Err(e) => win.set_status_message(format!("Error: {}", e).into()),
            }

            // If remapping was on, ensure it stays on after save.
            // (No-op; actual state lives in remapper.)
            let _ = remapper_clone.borrow();
        }
    });

    // Load profile callback
    let device_clone = device.clone();
    let remap_mappings_clone = remap_mappings.clone();
    let remapper_clone = remapper.clone();
    let dpi_poller_clone = dpi_poller.clone();
    let window_weak = window.as_weak();
    window.on_load_profile(move |profile_name| {
        info!("Loading profile: {}", profile_name);
        if let Some(win) = window_weak.upgrade() {
            let name = profile_name.to_string();
            if name.is_empty() {
                win.set_status_message("Enter a profile name first".into());
                return;
            }

            match ProfileManager::new() {
                Ok(manager) => {
                    match manager.load_profile(&name) {
                        Ok(profile) => {
                            // Update UI with profile settings
                            win.set_current_dpi_x(profile.dpi.x as i32);
                            win.set_current_dpi_y(profile.dpi.y as i32);

                            // Apply to device if connected
                            if let Some(ref mut dev) = *device_clone.borrow_mut() {
                                if let Err(e) = dev.set_dpi(profile.dpi.x, profile.dpi.y) {
                                    error!("Failed to apply profile DPI: {}", e);
                                }
                            }

                            // Load remap mappings into UI state
                            {
                                let mut map = remap_mappings_clone.borrow_mut();
                                map.clear();
                                for m in &profile.remap.mappings {
                                    map.insert(
                                        m.source,
                                        remap::MappingTarget {
                                            base: m.target,
                                            mods: remap::Modifiers {
                                                ctrl: m.ctrl,
                                                alt: m.alt,
                                                shift: m.shift,
                                                meta: m.meta,
                                            },
                                        },
                                    );
                                }
                            }
                            win.set_remap_enabled(profile.remap.enabled);
                            update_remap_summary(&win, &remap_mappings_clone.borrow());

                            // Start/stop remapper to match profile
                            if profile.remap.enabled {
                                start_remapper(&win, &device_clone, &remapper_clone, &remap_mappings_clone, &dpi_poller_clone);
                            } else {
                                stop_remapper(&device_clone, &remapper_clone, &dpi_poller_clone);
                            }

                            win.set_status_message(format!("Profile '{}' loaded!", name).into());
                        }
                        Err(e) => win.set_status_message(format!("Load error: {}", e).into()),
                    }
                }
                Err(e) => win.set_status_message(format!("Error: {}", e).into()),
            }
        }
    });

    // Remap enable/disable
    let window_weak = window.as_weak();
    let device_clone = device.clone();
    let remapper_clone = remapper.clone();
    let remap_mappings_clone = remap_mappings.clone();
    let dpi_poller_clone = dpi_poller.clone();
    window.on_remap_set_enabled(move |enabled| {
        if let Some(win) = window_weak.upgrade() {
            if enabled {
                start_remapper(&win, &device_clone, &remapper_clone, &remap_mappings_clone, &dpi_poller_clone);
            } else {
                stop_remapper(&device_clone, &remapper_clone, &dpi_poller_clone);
                win.set_status_message("Remapping disabled".into());
            }
        }
    });

    // Learn next button/key code (temporarily pause remapper so grabs don't block input)
    // Note: We use pause_remapper here to keep driver mode enabled, so side buttons can be learned
    let window_weak = window.as_weak();
    let remapper_clone = remapper.clone();
    window.on_remap_learn_source(move || {
        let was_enabled = remapper_clone.borrow().is_some();
        if was_enabled {
            pause_remapper(&remapper_clone);
            if let Some(win) = window_weak.upgrade() {
                win.set_remap_enabled(false);
                win.set_status_message("Paused remapping to learn source; press a button within 10s".into());
            }
        }

        let window_weak_inner = window_weak.clone();
        std::thread::spawn(move || {
            info!("Learn thread started, capturing next button press...");
            let result = remap::capture_next_key_code(Duration::from_secs(10), None);
            slint::invoke_from_event_loop(move || {
                if let Some(win) = window_weak_inner.upgrade() {
                    match result {
                        Ok(code) => {
                            info!("Learn captured code: {}", code);
                            win.set_remap_source_code(code as i32);
                            win.set_status_message(format!("Captured source code: {code}").into());
                        }
                        Err(e) => {
                            warn!("Learn failed: {}", e);
                            win.set_status_message(format!("Learn failed: {e}").into());
                        }
                    }
                }
            })
            .ok();
        });
    });

    // Update friendly target label
    let window_weak = window.as_weak();
    window.on_remap_update_target_label(move |code, ctrl, alt, shift, meta| {
        if let Some(win) = window_weak.upgrade() {
            let label = format_mapping_target(&remap::MappingTarget {
                base: code as u16,
                mods: remap::Modifiers {
                    ctrl,
                    alt,
                    shift,
                    meta,
                },
            });
            win.set_remap_target_label(label.into());
        }
    });

    // Add mapping
    let window_weak = window.as_weak();
    let remap_mappings_clone = remap_mappings.clone();
    window.on_remap_add_mapping(move |source, target, ctrl, alt, shift, meta| {
        if let Some(win) = window_weak.upgrade() {
            let s = source as u16;
            let t = target as u16;
            remap_mappings_clone.borrow_mut().insert(
                s,
                remap::MappingTarget {
                    base: t,
                    mods: remap::Modifiers {
                        ctrl,
                        alt,
                        shift,
                        meta,
                    },
                },
            );
            update_remap_summary(&win, &remap_mappings_clone.borrow());
            win.set_status_message(format!(
                "Mapped {} -> {}",
                s,
                format_mapping_target(&remap::MappingTarget {
                    base: t,
                    mods: remap::Modifiers {
                        ctrl,
                        alt,
                        shift,
                        meta,
                    },
                })
            )
            .into());
            
            // Reset source code and modifiers so user can configure next mapping cleanly
            win.set_remap_source_code(0);
            win.set_remap_mod_ctrl(false);
            win.set_remap_mod_alt(false);
            win.set_remap_mod_shift(false);
            win.set_remap_mod_meta(false);
            // Update the target label to reflect reset state
            win.invoke_remap_update_target_label(
                win.get_remap_target_code(),
                false,
                false,
                false,
                false,
            );
        }
    });

    // Clear mappings
    let window_weak = window.as_weak();
    let remap_mappings_clone = remap_mappings.clone();
    window.on_remap_clear(move || {
        if let Some(win) = window_weak.upgrade() {
            remap_mappings_clone.borrow_mut().clear();
            update_remap_summary(&win, &remap_mappings_clone.borrow());
            win.set_status_message("Mappings cleared".into());
        }
    });

    // Remove a single mapping by source code
    let window_weak = window.as_weak();
    let remap_mappings_clone = remap_mappings.clone();
    window.on_remap_remove_mapping(move |source| {
        if let Some(win) = window_weak.upgrade() {
            let s = source as u16;
            if remap_mappings_clone.borrow_mut().remove(&s).is_some() {
                update_remap_summary(&win, &remap_mappings_clone.borrow());
                win.set_status_message(format!("Removed mapping for button (code {})", s).into());
            } else {
                win.set_status_message(format!("No mapping found for code {}", s).into());
            }
        }
    });
}

fn start_remapper(
    win: &MainWindow,
    device: &Rc<RefCell<Option<device::RazerDevice>>>,
    remapper: &Rc<RefCell<Option<remap::Remapper>>>,
    mappings: &Rc<RefCell<BTreeMap<u16, remap::MappingTarget>>>,
    dpi_poller: &Rc<RefCell<Option<hidpoll::DpiButtonPoller>>>,
) {
    if remapper.borrow().is_some() {
        win.set_status_message("Remapping already enabled".into());
        return;
    }

    // Enable Driver Mode - this makes side buttons send keyboard keys
    // which can then be captured and remapped
    if let Some(ref mut dev) = *device.borrow_mut() {
        match dev.enable_driver_mode() {
            Ok(()) => {
                info!("Driver mode enabled for side button remapping");
            }
            Err(e) => {
                warn!("Failed to enable driver mode: {} - side buttons may not work", e);
                win.set_status_message(format!("Warning: Could not enable driver mode: {}", e).into());
            }
        }
    } else {
        warn!("No device connected - cannot enable driver mode");
    }

    let config = remap::RemapConfig {
        source_device: None,
        mappings: mappings.borrow().clone(),
    };

    // Start the DPI button poller FIRST so its virtual device exists
    // when the remapper enumerates devices
    if dpi_poller.borrow().is_none() {
        match hidpoll::DpiButtonPoller::start() {
            Ok(poller) => {
                info!("DPI button poller started");
                *dpi_poller.borrow_mut() = Some(poller);
                // Brief delay to let uinput device be created
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                warn!("Failed to start DPI poller: {} - DPI buttons won't be remappable", e);
            }
        }
    }

    match remap::Remapper::start(config) {
        Ok(r) => {
            *remapper.borrow_mut() = Some(r);
            win.set_status_message("Remapping enabled (virtual device active)".into());
        }
        Err(e) => {
            // If remapper fails, restore normal mode
            if let Some(ref mut dev) = *device.borrow_mut() {
                let _ = dev.disable_driver_mode();
            }
            // Also stop DPI poller if remapper fails
            if let Some(poller) = dpi_poller.borrow_mut().take() {
                poller.stop();
            }
            win.set_remap_enabled(false);
            win.set_status_message(format!("Remap start failed: {e}").into());
        }
    }
}

fn stop_remapper(
    device: &Rc<RefCell<Option<device::RazerDevice>>>,
    remapper: &Rc<RefCell<Option<remap::Remapper>>>,
    dpi_poller: &Rc<RefCell<Option<hidpoll::DpiButtonPoller>>>,
) {
    if let Some(r) = remapper.borrow_mut().take() {
        r.stop();
    }
    
    // Stop the DPI button poller
    if let Some(p) = dpi_poller.borrow_mut().take() {
        p.stop();
        info!("DPI button poller stopped");
    }

    // Disable Driver Mode - restore normal operation
    if let Some(ref mut dev) = *device.borrow_mut() {
        match dev.disable_driver_mode() {
            Ok(()) => {
                info!("Driver mode disabled - restored normal mode");
            }
            Err(e) => {
                warn!("Failed to disable driver mode: {}", e);
            }
        }
    }
}

/// Stop remapper without changing device mode (used when pausing for learning)
fn pause_remapper(remapper: &Rc<RefCell<Option<remap::Remapper>>>) {
    if let Some(r) = remapper.borrow_mut().take() {
        r.stop();
    }
}

/// Update the individual button mapping labels in the UI
/// Side buttons map to KEY_1=2 through KEY_EQUAL=13
/// Thumb buttons map to BTN_SIDE=275, BTN_EXTRA=276
fn update_button_mapping_labels(win: &MainWindow, mappings: &BTreeMap<u16, remap::MappingTarget>) {
    // Side button key codes in Driver Mode: KEY_1(2) through KEY_EQUAL(13)
    // Button 1 = KEY_1 = 2, Button 2 = KEY_2 = 3, ..., Button 12 = KEY_EQUAL = 13
    let get_mapping = |code: u16| -> String {
        mappings.get(&code)
            .map(|t| format_mapping_target(t))
            .unwrap_or_default()
    };
    
    win.set_btn1_mapping(get_mapping(2).into());   // KEY_1
    win.set_btn2_mapping(get_mapping(3).into());   // KEY_2
    win.set_btn3_mapping(get_mapping(4).into());   // KEY_3
    win.set_btn4_mapping(get_mapping(5).into());   // KEY_4
    win.set_btn5_mapping(get_mapping(6).into());   // KEY_5
    win.set_btn6_mapping(get_mapping(7).into());   // KEY_6
    win.set_btn7_mapping(get_mapping(8).into());   // KEY_7
    win.set_btn8_mapping(get_mapping(9).into());   // KEY_8
    win.set_btn9_mapping(get_mapping(10).into());  // KEY_9
    win.set_btn10_mapping(get_mapping(11).into()); // KEY_0
    win.set_btn11_mapping(get_mapping(12).into()); // KEY_MINUS
    win.set_btn12_mapping(get_mapping(13).into()); // KEY_EQUAL
    
    // Mouse buttons (only 3 exist on Naga Trinity: MIDDLE, SIDE, EXTRA)
    win.set_btn_middle_mapping(get_mapping(274).into());  // BTN_MIDDLE - scroll wheel click
    win.set_btn_side_mapping(get_mapping(275).into());    // BTN_SIDE - thumb back
    win.set_btn_extra_mapping(get_mapping(276).into());   // BTN_EXTRA - thumb forward
    
    // DPI buttons (captured via hidraw, injected as F13/F14)
    win.set_btn_dpi_down_mapping(get_mapping(184).into()); // KEY_F14 - DPI Down
    win.set_btn_dpi_up_mapping(get_mapping(183).into());   // KEY_F13 - DPI Up
}

fn update_remap_summary(win: &MainWindow, mappings: &BTreeMap<u16, remap::MappingTarget>) {
    // Update individual button mapping labels (side buttons are KEY_1=2 through KEY_EQUAL=13)
    update_button_mapping_labels(win, mappings);
    
    if mappings.is_empty() {
        win.set_remap_summary("No mappings".into());
        win.set_remap_mapping_details(
            "Click a side button to select it, then choose a target action.".into(),
        );
        return;
    }

    // Keep the one-line summary compact.
    let mut summary_parts: Vec<String> = mappings
        .iter()
        .take(6)
        .map(|(s, t)| format!("{s}->{}", format_mapping_target(t)))
        .collect();
    if mappings.len() > 6 {
        summary_parts.push(format!("+{} more", mappings.len() - 6));
    }
    win.set_remap_summary(summary_parts.join("  ").into());

    // Show a fuller list (truncated for readability) with guidance.
    let mut detail_lines: Vec<String> = mappings
        .iter()
        .take(12)
        .map(|(s, t)| format!("{s} → {}", format_mapping_target(t)))
        .collect();
    if mappings.len() > 12 {
        detail_lines.push(format!("+{} more mappings", mappings.len() - 12));
    }
    detail_lines.push("Tip: map side buttons to numbers or shortcuts you use often.".into());
    win.set_remap_mapping_details(detail_lines.join("\n").into());
}

fn format_mapping_target(t: &remap::MappingTarget) -> String {
    let mut parts: Vec<&'static str> = Vec::new();
    if t.mods.ctrl {
        parts.push("Ctrl");
    }
    if t.mods.alt {
        parts.push("Alt");
    }
    if t.mods.shift {
        parts.push("Shift");
    }
    if t.mods.meta {
        parts.push("Meta");
    }

    let base_name = key_name(t.base).unwrap_or_else(|| format!("KEY({})", t.base));
    if parts.is_empty() {
        base_name
    } else {
        format!("{}+{}", parts.join("+"), base_name)
    }
}

fn key_name(code: u16) -> Option<String> {
    // Common, user-friendly labels for typical keyboard codes
    match code {
        2..=11 => Some(format!("{}", code_to_digit(code)?)),
        59 => Some("F1".into()),
        60 => Some("F2".into()),
        61 => Some("F3".into()),
        62 => Some("F4".into()),
        63 => Some("F5".into()),
        64 => Some("F6".into()),
        65 => Some("F7".into()),
        66 => Some("F8".into()),
        67 => Some("F9".into()),
        68 => Some("F10".into()),
        69 => Some("F11".into()),
        70 => Some("F12".into()),
        28 => Some("Enter".into()),
        57 => Some("Space".into()),
        // Navigation keys
        102 => Some("Home".into()),
        103 => Some("Up".into()),
        104 => Some("Page Up".into()),
        105 => Some("Left".into()),
        106 => Some("Right".into()),
        107 => Some("End".into()),
        108 => Some("Down".into()),
        109 => Some("Page Down".into()),
        110 => Some("Insert".into()),
        111 => Some("Delete".into()),
        // Mouse buttons
        272 => Some("Left Click".into()),
        273 => Some("Right Click".into()),
        274 => Some("Middle Click".into()),
        275 => Some("Side (Back)".into()),
        276 => Some("Extra (Fwd)".into()),
        277 => Some("Forward".into()),
        278 => Some("Back".into()),
        279 => Some("BtnTask".into()),
        280 => Some("Scroll Up".into()),
        281 => Some("Scroll Down".into()),
        // Common keyboard keys
        30 => Some("A".into()),
        31 => Some("S".into()),
        44 => Some("Z".into()),
        45 => Some("X".into()),
        46 => Some("C".into()),
        47 => Some("V".into()),
        87 => Some("F11".into()),
        88 => Some("F12".into()),
        _ => None,
    }
}

fn code_to_digit(code: u16) -> Option<char> {
    // KEY_1..KEY_0 are 2..11
    match code {
        2 => Some('1'),
        3 => Some('2'),
        4 => Some('3'),
        5 => Some('4'),
        6 => Some('5'),
        7 => Some('6'),
        8 => Some('7'),
        9 => Some('8'),
        10 => Some('9'),
        11 => Some('0'),
        _ => None,
    }
}

// Helper function for use inside callbacks (can't use &MainWindow in closure)
fn connect_device_inner(window: &MainWindow, device: &Rc<RefCell<Option<device::RazerDevice>>>) {
    match device::find_naga_trinity() {
        Ok(Some(device_info)) => match device::RazerDevice::open(&device_info.path) {
            Ok(mut dev) => {
                window.set_device_name(device_info.product.into());
                window.set_device_connected(true);
                window.set_status_message("Connected".into());

                if let Ok(version) = dev.get_firmware_version() {
                    window.set_firmware_version(version.into());
                }

                if let Ok((dpi_x, dpi_y)) = dev.get_dpi() {
                    window.set_current_dpi_x(dpi_x as i32);
                    window.set_current_dpi_y(dpi_y as i32);
                }

                *device.borrow_mut() = Some(dev);
            }
            Err(e) => {
                window.set_status_message(format!("Error: {}", e).into());
            }
        },
        Ok(None) => {
            window.set_device_name("No device found".into());
            window.set_device_connected(false);
            window.set_status_message("No device found".into());
        }
        Err(e) => {
            window.set_status_message(format!("Scan error: {}", e).into());
        }
    }
}
