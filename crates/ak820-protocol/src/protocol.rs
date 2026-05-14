//! Low-level wire framing for the AK820 Pro.
//!
//! Decoded from the official AJAZZ online driver
//! (https://ajazz.driveall.cn — see `docs/reverse-engineering/online-driver/default-protocol.js`).
//! The upstream Linux ports (`gohv`, `TaxMachine`) use a completely different,
//! seemingly broken layout that the real firmware ignores on macOS — see
//! `docs/PROTOCOL.md` for the diff.

use crate::error::*;

pub type ReportId = u8;

/// Standard packet length on the AK820 Pro vendor endpoint (usage page 0xFF67).
/// The official driver reads this from `outputReports[0].items[0].reportCount`,
/// defaulting to 32 — for the AK820 Pro it's 64.
pub const PACKET_LEN: usize = 64;

/// Number of header bytes before the payload in an outgoing frame.
pub const HEADER_LEN: usize = 8;

/// Maximum payload bytes per packet (for chunking long transfers).
pub const PAYLOAD_PER_PACKET: usize = PACKET_LEN - HEADER_LEN;

/// Report ID prefix `device.sendReport(REPORT_ID, …)` uses. The online driver
/// uses `V = 0`.
pub const REPORT_ID: ReportId = 0;

/// Outgoing frames start with this magic byte; responses use 0x55.
pub const MAGIC_OUTGOING: u8 = 0xAA;
pub const MAGIC_INCOMING: u8 = 0x55;

/// Command bytes (subset — full list in docs/PROTOCOL.md).
#[allow(dead_code)]
pub mod cmd {
    pub const COMMUNICATION_START: u8 = 1;
    pub const COMMUNICATION_END: u8 = 2;
    pub const SET_FACTORY_RESET: u8 = 15;
    pub const GET_DEVICE_INFO: u8 = 16;
    pub const GET_GAME_MODE: u8 = 17;
    pub const GET_KEY: u8 = 18;
    pub const GET_LED_EFFECT: u8 = 19;
    pub const GET_CUSTOM_LED_DATA: u8 = 20;
    pub const GET_MACRO: u8 = 21;
    pub const GET_FN_KEY: u8 = 22;
    pub const SET_GAME_MODE: u8 = 33;
    pub const SET_KEY: u8 = 34;
    pub const SET_LED_EFFECT: u8 = 35;
    pub const SET_CUSTOM_LED_DATA: u8 = 36;
    pub const SET_MACRO: u8 = 37;
    pub const SET_FN_KEY: u8 = 38;
    pub const GET_DEFAULT_FN_KEY_MATRIX: u8 = 28;
    pub const GET_DEFAULT_KEY_MATRIX: u8 = 31;
    pub const SET_LED_BOOT_ANIMATION: u8 = 64;
    pub const SET_TFT_USER_ANIMATION: u8 = 80;
    pub const SET_TFT_BUILT_IN_INDEX: u8 = 81;
}

/// Build one outgoing frame, port of `P()` from the online driver.
///
/// Layout (64 bytes total):
/// `[0xAA, cmd, len_or_type, addr_lo, addr_hi, opt0, opt1_or_last_flag, opt2, payload…]`
pub fn build_frame(
    cmd: u8,
    len_or_type: u8,
    addr: u16,
    payload: &[u8],
    last_packet: bool,
) -> [u8; PACKET_LEN] {
    let mut pkt = [0u8; PACKET_LEN];
    pkt[0] = MAGIC_OUTGOING;
    pkt[1] = cmd;
    pkt[2] = len_or_type;
    pkt[3] = (addr & 0xFF) as u8;
    pkt[4] = ((addr >> 8) & 0xFF) as u8;
    pkt[6] = if last_packet { 1 } else { 0 };
    let n = payload.len().min(PAYLOAD_PER_PACKET);
    pkt[HEADER_LEN..HEADER_LEN + n].copy_from_slice(&payload[..n]);
    pkt
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub report_id: ReportId,
    pub data: Vec<u8>,
}

impl Frame {
    pub fn new(report_id: ReportId, data: impl Into<Vec<u8>>) -> Result<Self> {
        let data = data.into();
        if data.len() > PACKET_LEN {
            return Err(Error::FrameTooLong {
                len: data.len(),
                max: PACKET_LEN,
            });
        }
        Ok(Self { report_id, data })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PACKET_LEN + 1);
        out.push(self.report_id);
        out.extend_from_slice(&self.data);
        out.resize(PACKET_LEN + 1, 0);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_frame_short_payload() {
        let p = build_frame(cmd::SET_LED_EFFECT, 16, 0, &[0xDE, 0xAD], true);
        assert_eq!(p[0], 0xAA);
        assert_eq!(p[1], 35);
        assert_eq!(p[2], 16);
        assert_eq!(p[3], 0);
        assert_eq!(p[4], 0);
        assert_eq!(p[6], 1);
        assert_eq!(p[8], 0xDE);
        assert_eq!(p[9], 0xAD);
        assert!(p[10..].iter().all(|&b| b == 0));
    }

    #[test]
    fn build_frame_addr_split() {
        let p = build_frame(cmd::SET_KEY, 8, 0x1234, &[], false);
        assert_eq!(p[3], 0x34);
        assert_eq!(p[4], 0x12);
        assert_eq!(p[6], 0);
    }
}
