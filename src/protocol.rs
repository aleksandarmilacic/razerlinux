//! Razer USB HID Protocol implementation
//!
//! Based on reverse-engineering from OpenRazer project.
//!
//! Report Structure (90 bytes):
//! - Byte 0: Status (0x00 = new command, 0x01 = busy, 0x02 = success, 0x03 = failure, 0x05 = not supported)
//! - Byte 1: Transaction ID (0xFF for older devices, 0x3F for newer)
//! - Bytes 2-3: Remaining packets (u16 big-endian, 0x0000 for single packet)
//! - Byte 4: Protocol type (0x00)
//! - Byte 5: Data size
//! - Byte 6: Command class
//! - Byte 7: Command ID
//! - Bytes 8-87: Arguments (80 bytes)
//! - Byte 88: CRC (XOR of bytes 2-87)
//! - Byte 89: Reserved (0x00)

use anyhow::{Result, anyhow};

/// Variable storage types used by Razer devices
pub const VARSTORE: u8 = 0x01; // Store in device persistent memory
pub const NOSTORE: u8 = 0x00; // Don't store, temporary

/// Transaction IDs for different device types
pub const TRANSACTION_ID_OLD: u8 = 0xFF; // Older devices like Naga Trinity
pub const TRANSACTION_ID_NEW: u8 = 0x3F; // Newer Chroma devices  
pub const TRANSACTION_ID_WIRELESS: u8 = 0x1F; // Wireless devices (newer)

/// Command classes for Razer devices
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum CommandClass {
    General = 0x00,
    Led = 0x03,
    Mouse = 0x04,
}

/// Specific commands
#[derive(Debug, Clone, Copy)]
pub enum Command {
    // General commands
    GetFirmwareVersion,
    GetSerialNumber,
    GetPollingRate,
    SetPollingRate,
    GetDeviceMode,
    SetDeviceMode,

    // Mouse commands
    GetDpi,
    SetDpi,
}

impl Command {
    /// Get the command class and ID for this command
    pub fn class_and_id(&self) -> (u8, u8) {
        match self {
            // General commands (class 0x00)
            Command::GetFirmwareVersion => (0x00, 0x81),
            Command::GetSerialNumber => (0x00, 0x82),
            Command::GetPollingRate => (0x00, 0x85),
            Command::SetPollingRate => (0x00, 0x05),
            Command::GetDeviceMode => (0x00, 0x84),
            Command::SetDeviceMode => (0x00, 0x04),

            // Mouse commands (class 0x04)
            Command::GetDpi => (0x04, 0x85),
            Command::SetDpi => (0x04, 0x05),
        }
    }

    /// Get the expected data size for this command
    /// This is crucial - must match what OpenRazer uses!
    pub fn data_size(&self) -> u8 {
        match self {
            Command::GetFirmwareVersion => 0x02, // Returns 2 bytes (major, minor)
            Command::GetSerialNumber => 0x16,    // Returns 22-byte serial
            Command::GetPollingRate => 0x01,     // Returns 1 byte
            Command::SetPollingRate => 0x01,
            Command::GetDeviceMode => 0x02,      // Returns 2 bytes (mode, param)
            Command::SetDeviceMode => 0x02,      // Takes 2 bytes (mode, param)
            Command::GetDpi => 0x07, // CRITICAL: must be 0x07 for DPI query
            Command::SetDpi => 0x07,
        }
    }
}

/// A Razer HID report for communication
#[derive(Debug, Clone)]
pub struct RazerReport {
    pub status: u8,
    pub transaction_id: u8,
    pub remaining_packets: u16, // Big-endian u16!
    pub protocol_type: u8,
    pub data_size: u8,
    pub command_class: u8,
    pub command_id: u8,
    pub data: [u8; 80], // Arguments
}

impl RazerReport {
    /// Create a new report for a command
    pub fn new(command: Command) -> Self {
        let (class, id) = command.class_and_id();

        Self {
            status: 0x00,              // New command
            transaction_id: 0xFF,      // Naga Trinity uses 0xFF for DPI/old-style commands
            remaining_packets: 0x0000, // Single packet (u16)
            protocol_type: 0x00,
            data_size: command.data_size(),
            command_class: class,
            command_id: id,
            data: [0u8; 80],
        }
    }

    /// Create a new report for a command with specific transaction ID
    pub fn new_with_transaction_id(command: Command, transaction_id: u8) -> Self {
        let mut report = Self::new(command);
        report.transaction_id = transaction_id;
        report
    }

    /// Calculate CRC (XOR of bytes 2-87)
    fn calculate_crc(&self) -> u8 {
        let bytes = self.to_bytes_without_crc();
        bytes[2..88].iter().fold(0u8, |acc, &x| acc ^ x)
    }

    /// Convert to bytes without CRC (for CRC calculation)
    fn to_bytes_without_crc(&self) -> [u8; 90] {
        let mut bytes = [0u8; 90];
        bytes[0] = self.status;
        bytes[1] = self.transaction_id;
        // remaining_packets is big-endian u16 at bytes 2-3
        bytes[2] = (self.remaining_packets >> 8) as u8;
        bytes[3] = (self.remaining_packets & 0xFF) as u8;
        bytes[4] = self.protocol_type;
        bytes[5] = self.data_size;
        bytes[6] = self.command_class;
        bytes[7] = self.command_id;
        bytes[8..88].copy_from_slice(&self.data);
        bytes
    }

    /// Convert to bytes for sending
    pub fn to_bytes(&self) -> [u8; 90] {
        let mut bytes = self.to_bytes_without_crc();
        bytes[88] = self.calculate_crc(); // CRC at byte 88
        bytes[89] = 0x00; // Reserved at byte 89
        bytes
    }

    /// Parse a response from bytes
    pub fn from_bytes(bytes: &[u8; 90]) -> Result<Self> {
        let mut data = [0u8; 80];
        data.copy_from_slice(&bytes[8..88]); // Arguments at bytes 8-87

        let report = Self {
            status: bytes[0],
            transaction_id: bytes[1],
            remaining_packets: u16::from_be_bytes([bytes[2], bytes[3]]),
            protocol_type: bytes[4],
            data_size: bytes[5],
            command_class: bytes[6],
            command_id: bytes[7],
            data,
        };

        // Check status
        match report.status {
            0x02 => Ok(report), // Success
            0x01 => Err(anyhow!("Device busy")),
            0x03 => Err(anyhow!("Command failed")),
            0x04 => Err(anyhow!("Command timeout")),
            0x05 => Err(anyhow!("Command not supported")),
            _ => Ok(report), // Unknown status, try to continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_creation() {
        let report = RazerReport::new(Command::GetFirmwareVersion);
        assert_eq!(report.command_class, 0x00);
        assert_eq!(report.command_id, 0x81);
        assert_eq!(report.transaction_id, 0xFF);
    }

    #[test]
    fn test_report_serialization() {
        let report = RazerReport::new(Command::GetDpi);
        let bytes = report.to_bytes();
        assert_eq!(bytes[0], 0x00); // status
        assert_eq!(bytes[1], 0xFF); // transaction_id
        assert_eq!(bytes[2], 0x00); // remaining_packets high byte
        assert_eq!(bytes[3], 0x00); // remaining_packets low byte
        assert_eq!(bytes[4], 0x00); // protocol_type
        assert_eq!(bytes[5], 0x07); // data_size (DPI uses 7)
        assert_eq!(bytes[6], 0x04); // command_class
        assert_eq!(bytes[7], 0x85); // command_id
    }
}
