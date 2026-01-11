#!/usr/bin/env python3
"""Reset Razer Naga Trinity to Normal Mode"""
import usb.core
import time

def create_razer_report(command_class, command_id, data_size, data=None):
    report = bytearray(90)
    report[0] = 0x00
    report[1] = 0x1f
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

dev = usb.core.find(idVendor=0x1532, idProduct=0x0067)
if dev:
    # Set Normal Mode (0x00)
    report = create_razer_report(0x00, 0x04, 0x02, [0x00, 0x00])
    dev.ctrl_transfer(0x21, 0x09, 0x0300, 0x00, report, timeout=1000)
    time.sleep(0.1)
    print("Mouse reset to Normal Mode")
else:
    print("Device not found")
