use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("HID error: {0}")]
    Hid(#[from] hidapi::HidError),

    #[error("Device not found (VID=0x{vid:04x}, interface={interface})")]
    DeviceNotFound { vid: u16, interface: i32 },

    #[error("Frame too long: {len} bytes (max {max})")]
    FrameTooLong { len: usize, max: usize },

    #[error("Unexpected response: {0}")]
    UnexpectedResponse(String),

    #[error("Feature not yet implemented: {0}")]
    NotImplemented(&'static str),

    #[error("Value out of range for {field}: {value} (max {max})")]
    OutOfRange { field: &'static str, value: i64, max: i64 },

    #[error("Macro {macro_id} is too large: {size} bytes (limit {limit})")]
    MacroTooLarge { macro_id: u8, size: usize, limit: usize },
}

pub type Result<T> = std::result::Result<T, Error>;
