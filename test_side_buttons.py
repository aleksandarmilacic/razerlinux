#!/usr/bin/env python3
"""
Test Razer Naga Trinity side buttons in Driver Mode.
Enables Driver Mode, then listens for keyboard events.
"""
import usb.core
import time
import evdev

def create_razer_report(command_class, command_id, data_size, data=None):
    report = bytearray(90)
    report[0] = 0x00
    report[1] = 0x1f  # Transaction ID
    report[5] = data_size
    report[6] = command_class
    report[7] = command_id
    if data:
        for i, b in enumerate(data):
            report[8 + i] = b
    crc = 0
    for i in range(2, 88):
        crc ^= report[i]
    report[88] = crc
    return bytes(report)

def set_driver_mode(mode=0x03):
    """Set device to Driver Mode (0x03) or Normal Mode (0x00)"""
    dev = usb.core.find(idVendor=0x1532, idProduct=0x0067)
    if not dev:
        print("Device not found")
        return False
    
    report = create_razer_report(0x00, 0x04, 0x02, [mode, 0x00])
    dev.ctrl_transfer(0x21, 0x09, 0x0300, 0x00, report, timeout=1000)
    time.sleep(0.1)
    
    # Verify
    report = create_razer_report(0x00, 0x84, 0x02)
    dev.ctrl_transfer(0x21, 0x09, 0x0300, 0x00, report, timeout=1000)
    time.sleep(0.05)
    response = dev.ctrl_transfer(0xa1, 0x01, 0x0300, 0x00, 90, timeout=1000)
    actual_mode = response[8]
    print(f"Device mode: 0x{actual_mode:02x} ({'Driver' if actual_mode == 0x03 else 'Normal'})")
    return actual_mode == mode

def find_razer_keyboard():
    """Find the Razer keyboard interface"""
    for path in evdev.list_devices():
        dev = evdev.InputDevice(path)
        if 'Razer' in dev.name and 'Keyboard' in dev.name:
            return dev
    return None

def main():
    print("=== Razer Naga Trinity Side Button Test ===\n")
    
    print("1. Enabling Driver Mode...")
    if not set_driver_mode(0x03):
        print("Failed to enable Driver Mode")
        return
    
    print("\n2. Finding keyboard interface...")
    kbd = find_razer_keyboard()
    if not kbd:
        print("Keyboard interface not found")
        set_driver_mode(0x00)
        return
    
    print(f"   Found: {kbd.path} - {kbd.name}")
    
    print("\n3. Press each side button (1-12) one at a time. Press Ctrl+C when done.")
    print("   Side button -> Key mapping will be shown.\n")
    
    button_map = {}
    
    try:
        kbd.grab()  # Prevent keys from going to system
        for event in kbd.read_loop():
            if event.type == evdev.ecodes.EV_KEY:
                key_event = evdev.categorize(event)
                if event.value == 1:  # Key press
                    key_name = evdev.ecodes.KEY.get(event.code, f"KEY_{event.code}")
                    print(f"   Key pressed: {key_name} (code: {event.code})")
                    button_map[event.code] = key_name
    except KeyboardInterrupt:
        kbd.ungrab()
        print("\n\n=== Summary ===")
        print("Key codes detected from side buttons:")
        for code, name in sorted(button_map.items()):
            print(f"  {name}: {code}")
    finally:
        print("\n4. Resetting to Normal Mode...")
        set_driver_mode(0x00)
        print("Done!")

if __name__ == "__main__":
    main()
