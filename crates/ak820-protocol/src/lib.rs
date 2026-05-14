//! Core protocol library for the Epomaker / Ajazz AK820 Pro keyboard.
//!
//! Phase 0 scope: device enumeration, HID connection, framing primitives, probe command.
//! Phase 1 scope: lighting transaction (apply 20 modes with RGB + rainbow + brightness +
//! speed + direction).

pub mod commands;
pub mod device;
pub mod error;
pub mod protocol;

pub use device::{enumerate, probe_interfaces, Connection, DeviceInfo, InterfaceProbe};
pub use error::{Error, Result};
pub use protocol::{Frame, ReportId};

/// Sonix Technology Co. Ltd. — confirmed against the live device.
pub const VENDOR_ID: u16 = 0x0C45;

/// Known product IDs for the AK820 / AK820 Pro family.
pub const PRODUCT_IDS: &[u16] = &[
    0x8009, // wired + 2.4 GHz dongle mode
    0xFEFE, // Bluetooth 5.1 mode (separate HID stack)
    0x7140, // ISP/bootloader mode (do not poke without intent)
];

pub const CONTROL_INTERFACE: i32 = 3;
