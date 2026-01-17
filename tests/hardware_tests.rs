//! Hardware-dependent tests that require a real Razer device
//! 
//! These tests are ignored by default and can be run with:
//! `cargo test -- --ignored`
//!
//! They require:
//! - A connected Razer Naga Trinity
//! - Root/sudo permissions for HID access
//! - uinput module loaded

/// Test device detection with real hardware
#[test]
#[ignore]
fn test_real_device_detection() {
    // This test requires actual hardware
    // Run with: sudo cargo test -- --ignored test_real_device_detection
    
    use std::process::Command;
    
    let output = Command::new("lsusb")
        .output()
        .expect("Failed to run lsusb");
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Check if any Razer device is connected
    if stdout.contains("1532:") {
        println!("Razer device found in USB devices");
    } else {
        panic!("No Razer device found. Connect a Razer device to run this test.");
    }
}

/// Test HID access with real hardware
#[test]
#[ignore]
fn test_real_hid_access() {
    use std::fs;
    
    // Look for hidraw devices
    let hidraw_devices: Vec<_> = fs::read_dir("/dev")
        .expect("Can't read /dev")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("hidraw"))
        .collect();
    
    println!("Found {} hidraw devices", hidraw_devices.len());
    assert!(!hidraw_devices.is_empty(), "No hidraw devices found");
}

/// Test evdev keyboard detection
#[test]
#[ignore]
fn test_real_evdev_keyboards() {
    use std::fs;
    
    let input_devices: Vec<_> = fs::read_dir("/dev/input")
        .expect("Can't read /dev/input")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("event"))
        .collect();
    
    println!("Found {} input event devices", input_devices.len());
    assert!(!input_devices.is_empty(), "No input event devices found");
}

/// Test uinput availability
#[test]
#[ignore]
fn test_real_uinput_available() {
    use std::path::Path;
    
    let uinput_path = Path::new("/dev/uinput");
    assert!(uinput_path.exists(), "/dev/uinput not found. Load the uinput module with: sudo modprobe uinput");
}

/// Test DPI read from real device
#[test]
#[ignore]
fn test_real_dpi_read() {
    // This would require importing the device module
    // For now, just verify we can access the hidraw device
    use std::fs::File;
    
    // Try to find a Razer hidraw device
    for i in 0..20 {
        let path = format!("/dev/hidraw{}", i);
        if let Ok(_file) = File::open(&path) {
            println!("Opened {}", path);
            // In real test, we'd send a DPI query
            break;
        }
    }
}

/// Test macro recording with real keyboard
#[test]
#[ignore]
fn test_real_macro_recording() {
    println!("This test would capture real keyboard input.");
    println!("Run interactively with a keyboard to test key capture.");
    // In a real interactive test, we'd start recording and capture keys
}
