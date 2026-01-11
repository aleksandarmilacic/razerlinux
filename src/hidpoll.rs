//! HID Raw Device Polling for DPI Buttons
//!
//! The Razer Naga Trinity's DPI buttons (under the scroll wheel) don't generate
//! standard Linux input events. Instead, they send special HID reports on the
//! keyboard interface that are only visible via hidraw.
//!
//! This module polls the hidraw device for these special reports (Report ID 0x04)
//! and converts DPI button codes to virtual F13/F14 key events that can be
//! remapped like any other button.
//!
//! Based on reverse-engineering from OpenRazer kernel driver:
//! - Report format: 0x04 [modifiers] [key codes...]
//! - DPI Up:   code 0x20 -> F13 (keycode 183)
//! - DPI Down: code 0x21 -> F14 (keycode 184)

use anyhow::{Context, Result};
use evdev::{EventType, InputEvent, uinput::VirtualDeviceBuilder, AttributeSet, Key};
use std::fs::File;
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;
use tracing::{info, warn, debug};

/// Razer USB VID
const RAZER_VID: u16 = 0x1532;
/// Naga Trinity PID
const NAGA_TRINITY_PID: u16 = 0x0067;

/// HID report codes for DPI buttons (from OpenRazer)
const HID_CODE_DPI_UP: u8 = 0x20;   // M1 in OpenRazer terminology
const HID_CODE_DPI_DOWN: u8 = 0x21; // M2 in OpenRazer terminology

/// Linux F13/F14 key codes (KEY_F13=183, KEY_F14=184)
const KEY_F13: u16 = 183;
const KEY_F14: u16 = 184;

/// Find all hidraw devices for the Razer Naga Trinity keyboard interface
pub fn find_naga_trinity_hidraw_devices() -> Vec<PathBuf> {
    let mut devices = Vec::new();
    
    // Scan /sys/class/hidraw/ for Razer devices
    let hidraw_class = std::path::Path::new("/sys/class/hidraw");
    
    if !hidraw_class.exists() {
        warn!("hidraw class not found at /sys/class/hidraw");
        return devices;
    }
    
    if let Ok(entries) = std::fs::read_dir(hidraw_class) {
        for entry in entries.flatten() {
            let hidraw_name = entry.file_name();
            let hidraw_name_str = hidraw_name.to_string_lossy();
            
            // Check device info through sysfs
            let device_path = entry.path().join("device");
            let uevent_path = device_path.join("uevent");
            
            if let Ok(uevent) = std::fs::read_to_string(&uevent_path) {
                // Parse MODALIAS or HID_ID to find our device
                // Format: HID_ID=0003:00001532:00000067
                let is_naga_trinity = uevent.lines().any(|line| {
                    if let Some(hid_id) = line.strip_prefix("HID_ID=") {
                        // Parse format: BUS:VID:PID
                        let parts: Vec<&str> = hid_id.split(':').collect();
                        if parts.len() >= 3 {
                            if let (Ok(vid), Ok(pid)) = (
                                u16::from_str_radix(parts[1], 16),
                                u16::from_str_radix(parts[2], 16)
                            ) {
                                return vid == RAZER_VID && pid == NAGA_TRINITY_PID;
                            }
                        }
                    }
                    false
                });
                
                if is_naga_trinity {
                    let dev_path = PathBuf::from("/dev").join(&hidraw_name_str.as_ref());
                    info!("Found Naga Trinity hidraw device: {:?}", dev_path);
                    devices.push(dev_path);
                }
            }
        }
    }
    
    devices
}

/// DPI Button Poller - polls hidraw for DPI button HID reports
pub struct DpiButtonPoller {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl DpiButtonPoller {
    /// Start polling for DPI button events
    /// 
    /// This creates a background thread that:
    /// 1. Opens all Naga Trinity hidraw devices
    /// 2. Polls for Report ID 0x04 (keyboard report with special keys)
    /// 3. Converts DPI button codes (0x20/0x21) to F13/F14 key events
    /// 4. Injects those events via uinput virtual device
    pub fn start() -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        
        let join = thread::spawn(move || {
            if let Err(e) = run_dpi_poller_loop(stop_thread) {
                warn!("DPI button poller stopped: {e:#}");
            }
        });
        
        Ok(Self {
            stop,
            join: Some(join),
        })
    }
    
    /// Stop the poller
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for DpiButtonPoller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

fn run_dpi_poller_loop(stop: Arc<AtomicBool>) -> Result<()> {
    let hidraw_devices = find_naga_trinity_hidraw_devices();
    
    if hidraw_devices.is_empty() {
        warn!("No Naga Trinity hidraw devices found - DPI buttons won't be available");
        // Keep thread alive but just sleep until stopped
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(500));
        }
        return Ok(());
    }
    
    info!("DPI poller: found {} hidraw device(s)", hidraw_devices.len());
    
    // Open all hidraw devices
    let mut files: Vec<(File, PathBuf)> = Vec::new();
    for path in &hidraw_devices {
        match File::open(path) {
            Ok(file) => {
                // Set non-blocking mode
                let fd = file.as_raw_fd();
                let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
                if flags >= 0 {
                    unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
                }
                info!("DPI poller: opened {:?}", path);
                files.push((file, path.clone()));
            }
            Err(e) => {
                warn!("DPI poller: failed to open {:?}: {}", path, e);
            }
        }
    }
    
    if files.is_empty() {
        warn!("DPI poller: could not open any hidraw devices (check permissions?)");
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(500));
        }
        return Ok(());
    }
    
    // Create virtual keyboard device for injecting F13/F14 events
    let mut keys = AttributeSet::<Key>::new();
    keys.insert(Key::new(KEY_F13));
    keys.insert(Key::new(KEY_F14));
    
    let vbuilder = VirtualDeviceBuilder::new()
        .context("Failed to create uinput builder for DPI buttons")?
        .name("RazerLinux DPI Buttons")
        .with_keys(&keys)
        .context("Failed to set F13/F14 key capabilities")?;
    
    let mut vdev = vbuilder.build()
        .context("Failed to build uinput device for DPI buttons")?;
    
    info!("DPI poller: virtual keyboard created, polling for DPI button reports...");
    
    // Track button states to detect press/release
    let mut dpi_up_pressed = false;
    let mut dpi_down_pressed = false;
    
    while !stop.load(Ordering::Relaxed) {
        let mut had_data = false;
        
        for (file, path) in &mut files {
            let mut buf = [0u8; 64]; // HID reports are typically up to 64 bytes
            
            match file.read(&mut buf) {
                Ok(len) if len > 0 => {
                    had_data = true;
                    
                    // Log ALL data we receive for debugging
                    let hex_str: String = buf[..len].iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    info!("DPI poller: {} bytes from {:?}: {}", len, path, hex_str);
                    
                    // Scan ALL bytes for DPI codes, regardless of report format
                    // This helps us discover where the codes actually appear
                    let mut dpi_up_positions: Vec<usize> = Vec::new();
                    let mut dpi_down_positions: Vec<usize> = Vec::new();
                    
                    for (i, &b) in buf[..len].iter().enumerate() {
                        if b == HID_CODE_DPI_UP {
                            dpi_up_positions.push(i);
                        } else if b == HID_CODE_DPI_DOWN {
                            dpi_down_positions.push(i);
                        }
                    }
                    
                    if !dpi_up_positions.is_empty() {
                        info!("DPI poller: Found DPI UP (0x20) at positions: {:?}", dpi_up_positions);
                    }
                    if !dpi_down_positions.is_empty() {
                        info!("DPI poller: Found DPI DOWN (0x21) at positions: {:?}", dpi_down_positions);
                    }
                    
                    // Check for Report ID 0x04 (keyboard report with special keys)
                    // Format WITH report ID: 0x04 [modifier] [reserved] [key1] [key2] ... [key6]
                    // Format WITHOUT report ID (some interfaces strip it): [modifier] [reserved] [key1] ...
                    
                    let found_dpi_up = !dpi_up_positions.is_empty();
                    let found_dpi_down = !dpi_down_positions.is_empty();
                    
                    // DPI Up press/release
                    if found_dpi_up && !dpi_up_pressed {
                        info!("DPI UP pressed -> injecting F13");
                        dpi_up_pressed = true;
                        let press = InputEvent::new(EventType::KEY, KEY_F13, 1);
                        let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                        if let Err(e) = vdev.emit(&[press, sync]) {
                            warn!("Failed to emit F13 press: {}", e);
                        }
                    } else if !found_dpi_up && dpi_up_pressed {
                        info!("DPI UP released -> injecting F13 release");
                        dpi_up_pressed = false;
                        let release = InputEvent::new(EventType::KEY, KEY_F13, 0);
                        let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                        if let Err(e) = vdev.emit(&[release, sync]) {
                            warn!("Failed to emit F13 release: {}", e);
                        }
                    }
                    
                    // DPI Down press/release
                    if found_dpi_down && !dpi_down_pressed {
                        info!("DPI DOWN pressed -> injecting F14");
                        dpi_down_pressed = true;
                        let press = InputEvent::new(EventType::KEY, KEY_F14, 1);
                        let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                        if let Err(e) = vdev.emit(&[press, sync]) {
                            warn!("Failed to emit F14 press: {}", e);
                        }
                    } else if !found_dpi_down && dpi_down_pressed {
                        info!("DPI DOWN released -> injecting F14 release");
                        dpi_down_pressed = false;
                        let release = InputEvent::new(EventType::KEY, KEY_F14, 0);
                        let sync = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                        if let Err(e) = vdev.emit(&[release, sync]) {
                            warn!("Failed to emit F14 release: {}", e);
                        }
                    }
                }
                Ok(_) => {
                    // Empty read
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available
                }
                Err(e) => {
                    warn!("DPI poller: read error from {:?}: {}", path, e);
                }
            }
        }
        
        if !had_data {
            thread::sleep(Duration::from_millis(5));
        }
    }
    
    info!("DPI poller: shutting down");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_find_hidraw() {
        // This test just runs the discovery - won't find devices unless run on actual hardware
        let devices = find_naga_trinity_hidraw_devices();
        println!("Found {} hidraw devices", devices.len());
        for d in &devices {
            println!("  {:?}", d);
        }
    }
}
