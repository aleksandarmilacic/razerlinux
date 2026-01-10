//! RazerLinux - Razer Mouse Configuration Tool
//! 
//! A userspace application for configuring Razer mice on Linux
//! without requiring kernel drivers.

mod device;
mod profile;
mod protocol;

use anyhow::Result;
use profile::{Profile, ProfileManager};
use std::cell::RefCell;
use std::rc::Rc;
use tracing::{info, error, warn};

slint::include_modules!();

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    info!("RazerLinux starting...");
    
    // Create the main window
    let main_window = MainWindow::new()?;
    
    // Shared device state
    let device: Rc<RefCell<Option<device::RazerDevice>>> = Rc::new(RefCell::new(None));
    
    // Try to find and connect to device on startup
    connect_device(&main_window, &device);
    
    // Setup callbacks
    setup_callbacks(&main_window, device);
    
    // Run the GUI event loop
    info!("Starting GUI...");
    main_window.run()?;
    
    info!("RazerLinux shutting down");
    Ok(())
}

fn connect_device(
    window: &MainWindow, 
    device: &Rc<RefCell<Option<device::RazerDevice>>>
) {
    match device::find_naga_trinity() {
        Ok(Some(device_info)) => {
            info!("Found Razer Naga Trinity at {}", device_info.path);
            
            match device::RazerDevice::open(&device_info.path) {
                Ok(mut dev) => {
                    info!("Device opened successfully!");
                    
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
    device: Rc<RefCell<Option<device::RazerDevice>>>
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
            let profile = Profile::from_device_settings(&name, dpi_x, dpi_y);
            
            match ProfileManager::new() {
                Ok(manager) => {
                    match manager.save_profile(&profile) {
                        Ok(_) => win.set_status_message(format!("Profile '{}' saved!", name).into()),
                        Err(e) => win.set_status_message(format!("Save error: {}", e).into()),
                    }
                }
                Err(e) => win.set_status_message(format!("Error: {}", e).into()),
            }
        }
    });
    
    // Load profile callback
    let device_clone = device.clone();
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
                            
                            win.set_status_message(format!("Profile '{}' loaded!", name).into());
                        }
                        Err(e) => win.set_status_message(format!("Load error: {}", e).into()),
                    }
                }
                Err(e) => win.set_status_message(format!("Error: {}", e).into()),
            }
        }
    });
}

// Helper function for use inside callbacks (can't use &MainWindow in closure)
fn connect_device_inner(
    window: &MainWindow,
    device: &Rc<RefCell<Option<device::RazerDevice>>>
) {
    match device::find_naga_trinity() {
        Ok(Some(device_info)) => {
            match device::RazerDevice::open(&device_info.path) {
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
            }
        }
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
