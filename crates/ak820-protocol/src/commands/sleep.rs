//! Sleep-timer command. Phase 2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SleepTimer {
    Never,
    OneMinute,
    FiveMinutes,
    ThirtyMinutes,
}
