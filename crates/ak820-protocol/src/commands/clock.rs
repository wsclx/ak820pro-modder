//! Clock sync for the TFT's stock clock page.
//!
//! The AJAZZ web driver's `setTftDateTime` sends a 10-byte payload via
//! `SET_TEMPORARY_COMMAND_DATA` (cmd 52):
//!
//! ```text
//! [0x5A, 0x01, 0x5A, year % 100, month, day, hour, minute, second, weekday]
//! ```
//!
//! Month is 1–12, weekday is Sunday = 0 … Saturday = 6 (JS `Date.getDay()`).
//! The same `01 5A YY MM DD hh mm ss` core shows up in the feature-report
//! clock packet of newer board revisions (`04 18` → `04 28` → data →
//! `04 02`), which is decent evidence the field layout is right. Not yet
//! hardware-verified on a firmware-1.07 board.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::*;

/// Calendar fields the TFT clock accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TftDateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// Sunday = 0 through Saturday = 6.
    pub weekday: u8,
}

impl TftDateTime {
    /// Current local wall-clock time. Errors instead of silently falling
    /// back to UTC when the local offset can't be determined: a clock
    /// that's off by the timezone is worse than a visible error.
    pub fn now_local() -> Result<Self> {
        let now = OffsetDateTime::now_local().map_err(|e| {
            Error::UnexpectedResponse(format!("can't determine local time offset: {e}"))
        })?;
        Ok(Self::from(now))
    }

    /// Encode the 10-byte `SET_TEMPORARY_COMMAND_DATA` payload.
    pub fn encode(&self) -> Result<[u8; 10]> {
        let in_range = (2000..=2099).contains(&self.year)
            && (1..=12).contains(&self.month)
            && (1..=31).contains(&self.day)
            && self.hour <= 23
            && self.minute <= 59
            && self.second <= 59
            && self.weekday <= 6;
        if !in_range {
            return Err(Error::UnexpectedResponse(format!(
                "TFT clock fields out of range: {self:?}"
            )));
        }
        Ok([
            0x5A,
            0x01,
            0x5A,
            (self.year % 100) as u8,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.weekday,
        ])
    }
}

impl From<OffsetDateTime> for TftDateTime {
    fn from(t: OffsetDateTime) -> Self {
        Self {
            year: t.year().clamp(0, u16::MAX as i32) as u16,
            month: u8::from(t.month()),
            day: t.day(),
            hour: t.hour(),
            minute: t.minute(),
            second: t.second(),
            weekday: t.weekday().number_days_from_sunday(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn encodes_web_driver_payload() {
        // Saturday, 2026-09-12 19:07:45.
        let t = TftDateTime::from(datetime!(2026-09-12 19:07:45 UTC));
        assert_eq!(t.weekday, 6);
        assert_eq!(
            t.encode().unwrap(),
            [0x5A, 0x01, 0x5A, 26, 9, 12, 19, 7, 45, 6]
        );
    }

    #[test]
    fn sunday_is_weekday_zero() {
        let t = TftDateTime::from(datetime!(2026-10-11 00:00:00 UTC));
        assert_eq!(t.weekday, 0);
    }

    #[test]
    fn rejects_out_of_range_fields() {
        let mut t = TftDateTime::from(datetime!(2026-01-01 00:00:00 UTC));
        t.month = 13;
        assert!(t.encode().is_err());
        let mut t = TftDateTime::from(datetime!(2026-01-01 00:00:00 UTC));
        t.year = 2100;
        assert!(t.encode().is_err());
    }
}
