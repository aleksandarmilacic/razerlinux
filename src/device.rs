//! Device detection and communication module

use crate::protocol::{Command, NOSTORE, RazerReport, VARSTORE};
use anyhow::{Context, Result};
use hidapi::HidApi;

/// Razer USB Vendor ID
pub const RAZER_VENDOR_ID: u16 = 0x1532;

/// Razer Naga Trinity Product ID
pub const NAGA_TRINITY_PID: u16 = 0x0067;

/// Information about a detected Razer device
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: String,
    pub product: String,
    pub interface_number: i32,
}

/// Find a Razer Naga Trinity device
pub fn find_naga_trinity() -> Result<Option<DeviceInfo>> {
    let api = HidApi::new().context("Failed to initialize HID API")?;

    // Debug: list all Naga Trinity interfaces
    for device in api.device_list() {
        if device.vendor_id() == RAZER_VENDOR_ID && device.product_id() == NAGA_TRINITY_PID {
            tracing::debug!(
                "Found Naga Trinity interface {}: {:?} (usage_page: {:#06x}, usage: {:#06x})",
                device.interface_number(),
                device.path().to_string_lossy(),
                device.usage_page(),
                device.usage()
            );
        }
    }

    // Try interfaces in order of preference for control messages
    // Interface 0 is typically the control interface for older Razer mice like Naga Trinity
    // Newer mice may use interface 2 or 3
    for preferred_interface in [0, 2, 1] {
        for device in api.device_list() {
            if device.vendor_id() == RAZER_VENDOR_ID && device.product_id() == NAGA_TRINITY_PID {
                if device.interface_number() == preferred_interface {
                    return Ok(Some(DeviceInfo {
                        path: device.path().to_string_lossy().to_string(),
                        vendor_id: device.vendor_id(),
                        product_id: device.product_id(),
                        manufacturer: device.manufacturer_string().unwrap_or_default().to_string(),
                        product: device.product_string().unwrap_or_default().to_string(),
                        interface_number: device.interface_number(),
                    }));
                }
            }
        }
    }

    Ok(None)
}

/// List all connected Razer devices
pub fn list_razer_devices() -> Result<Vec<DeviceInfo>> {
    let api = HidApi::new().context("Failed to initialize HID API")?;
    let mut devices = Vec::new();

    for device in api.device_list() {
        if device.vendor_id() == RAZER_VENDOR_ID {
            devices.push(DeviceInfo {
                path: device.path().to_string_lossy().to_string(),
                vendor_id: device.vendor_id(),
                product_id: device.product_id(),
                manufacturer: device.manufacturer_string().unwrap_or_default().to_string(),
                product: device.product_string().unwrap_or_default().to_string(),
                interface_number: device.interface_number(),
            });
        }
    }

    Ok(devices)
}

/// Handle to an open Razer device for communication
pub struct RazerDevice {
    handle: hidapi::HidDevice,
    #[allow(dead_code)]
    product_id: u16,
}

impl RazerDevice {
    /// Open a Razer device by path
    pub fn open(path: &str) -> Result<Self> {
        let api = HidApi::new().context("Failed to initialize HID API")?;
        let handle = api
            .open_path(std::ffi::CString::new(path)?.as_c_str())
            .context("Failed to open HID device")?;

        Ok(Self {
            handle,
            product_id: NAGA_TRINITY_PID,
        })
    }

    /// Send a command and receive a response
    fn send_command(&mut self, report: &RazerReport) -> Result<RazerReport> {
        let mut send_data = [0u8; 90];
        send_data.copy_from_slice(&report.to_bytes());

        // Debug: print what we're sending
        tracing::debug!("Sending (90 bytes): {:02x?}", &send_data[0..12]);

        // Send as feature report (report ID 0x00)
        // Prepend report ID for hidapi
        let mut with_report_id = [0u8; 91];
        with_report_id[0] = 0x00;
        with_report_id[1..91].copy_from_slice(&send_data);

        self.handle
            .send_feature_report(&with_report_id)
            .context("Failed to send feature report")?;

        // Wait for device to process - Razer devices need time
        std::thread::sleep(std::time::Duration::from_millis(80));

        // Read the response as feature report
        let mut response = [0u8; 91];
        response[0] = 0x00; // Report ID we want to read

        let len = self
            .handle
            .get_feature_report(&mut response)
            .context("Failed to get feature report")?;

        tracing::debug!("Read {} bytes, response: {:02x?}", len, &response[0..12]);

        // Parse response (skip report ID byte)
        let mut resp_data = [0u8; 90];
        resp_data.copy_from_slice(&response[1..91]);

        // Check if we got actual data back
        if resp_data[0] == 0x00 && resp_data[1] == 0x00 && resp_data[2] == 0x00 {
            // Response looks empty - might need to retry or the device didn't respond
            tracing::warn!("Device returned empty response - command may not be supported");
        }

        RazerReport::from_bytes(&resp_data)
    }

    /// Get the firmware version
    pub fn get_firmware_version(&mut self) -> Result<String> {
        let report = RazerReport::new(Command::GetFirmwareVersion);
        let response = self.send_command(&report)?;

        // Debug: print raw response
        tracing::debug!("Firmware response status: {:#04x}", response.status);
        tracing::debug!(
            "Firmware response data[0..8]: {:02x?}",
            &response.data[0..8]
        );

        // Firmware version is in the response data
        let major = response.data[0];
        let minor = response.data[1];

        Ok(format!("v{}.{}", major, minor))
    }

    /// Get the current DPI setting
    pub fn get_dpi(&mut self) -> Result<(u16, u16)> {
        let mut report = RazerReport::new(Command::GetDpi);

        // Set NOSTORE in the first argument byte (like OpenRazer does for Naga Trinity)
        report.data[0] = NOSTORE;

        let response = self.send_command(&report)?;

        // Debug: print raw response
        tracing::debug!("DPI response status: {:#04x}", response.status);
        tracing::debug!("DPI response data[0..10]: {:02x?}", &response.data[0..10]);

        // DPI is stored as big-endian u16 values
        // data[0] = variable storage (echo of what we sent)
        // data[1..2] = DPI X
        // data[3..4] = DPI Y
        let dpi_x = u16::from_be_bytes([response.data[1], response.data[2]]);
        let dpi_y = u16::from_be_bytes([response.data[3], response.data[4]]);

        Ok((dpi_x, dpi_y))
    }

    /// Set the DPI
    pub fn set_dpi(&mut self, dpi_x: u16, dpi_y: u16) -> Result<()> {
        let mut report = RazerReport::new(Command::SetDpi);

        // Variable storage: VARSTORE saves to device, NOSTORE is temporary
        report.data[0] = VARSTORE;
        // DPI values as big-endian
        report.data[1] = (dpi_x >> 8) as u8;
        report.data[2] = (dpi_x & 0xFF) as u8;
        report.data[3] = (dpi_y >> 8) as u8;
        report.data[4] = (dpi_y & 0xFF) as u8;
        report.data[5] = 0x00; // Reserved
        report.data[6] = 0x00; // Reserved

        let _response = self.send_command(&report)?;
        Ok(())
    }

    /// Get the polling rate
    pub fn get_polling_rate(&mut self) -> Result<u16> {
        let report = RazerReport::new(Command::GetPollingRate);
        let response = self.send_command(&report)?;

        // Polling rate is returned as interval in ms, convert to Hz
        let interval = response.data[0] as u16;
        let rate = if interval > 0 { 1000 / interval } else { 1000 };

        Ok(rate)
    }

    /// Set the polling rate (125, 500, or 1000 Hz)
    pub fn set_polling_rate(&mut self, rate: u16) -> Result<()> {
        let interval = match rate {
            125 => 8,  // 8ms interval
            500 => 2,  // 2ms interval
            1000 => 1, // 1ms interval
            _ => {
                return Err(anyhow::anyhow!(
                    "Invalid polling rate. Use 125, 500, or 1000"
                ));
            }
        };

        let mut report = RazerReport::new(Command::SetPollingRate);
        report.data[0] = interval;
        report.data_size = 1;

        let _response = self.send_command(&report)?;
        Ok(())
    }
}
