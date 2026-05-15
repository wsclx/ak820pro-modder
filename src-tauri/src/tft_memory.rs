//! Remember the last-applied TFT state so we can re-apply it after
//! commands that the firmware treats as side-effecting.
//!
//! ## Why this exists
//!
//! Mario reported that applying a Lighting effect resets the TFT panel
//! back to its built-in factory animation, wiping whatever user content
//! we had uploaded. The official AJAZZ web driver sends the exact same
//! 16-byte `SET_LED_EFFECT` payload we do — same wire bytes, same cmd
//! — so the trigger isn't our code, it's the firmware. Without
//! firmware source we can't fix it at the source; the next-best move
//! is to remember what TFT content was last requested and re-upload
//! it after every command that gets observed to wipe the panel.
//!
//! ## What's tracked
//!
//! Either a preset id (cheap to re-build by name) or the raw
//! upload-image bytes + fit mode (so we can re-quantise + re-upload
//! the same custom image without going back to the filesystem). When
//! the user explicitly calls `tft_factory_default`, we *clear* the
//! memory so we don't keep re-applying old state.
//!
//! ## What's NOT tracked
//!
//! `setTftDateTime` / `setTftScreenInfo` — these aren't full-frame
//! uploads, they're per-call data pushes for the firmware's date /
//! stats overlay screens. If we ever ship live-stat presets (Phase
//! 5e), the polling loop owns its own state; this module stays
//! focused on the user-animation slot.
//!
//! ## When `restore_after_side_effect` runs
//!
//! Only the `apply_lighting` and `apply_preset` commands trigger
//! restore today, because those are the ones Mario observed wiping
//! the TFT. Adding more triggers is one-liner additions in `lib.rs`
//! — if testing reveals SET_KEY / SET_MACRO / SET_GAME_MODE / etc.
//! also wipe the panel, hook them here.

use ak820_protocol::commands::tft_image::{self, FitMode};
use ak820_protocol::commands::tft_presets;
use ak820_protocol::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::AppError;

/// What the user last asked the TFT to show. `None` = nothing to
/// restore (either the user explicitly hit Factory Default, or they've
/// never touched the TFT this session).
#[derive(Debug, Clone)]
pub enum TftMemoryState {
    /// Re-build by id from `tft_presets::build`. Stored as a String so
    /// the catalogue can grow without breaking persistence later.
    Preset(String),
    /// Raw uploaded image bytes (PNG / JPEG / GIF) + the user's chosen
    /// fit mode. Cached in memory so we don't have to seek back to disk
    /// to re-decode — files can be huge, but a single GIF is usually
    /// a few MB at most. Cleared on factory-default or explicit user
    /// reset; never persisted to disk.
    Image { bytes: Vec<u8>, fit: FitMode },
}

#[derive(Default)]
pub struct TftMemory(Mutex<Option<TftMemoryState>>);

impl TftMemory {
    pub async fn remember_preset(&self, id: String) {
        *self.0.lock().await = Some(TftMemoryState::Preset(id));
    }

    pub async fn remember_image(&self, bytes: Vec<u8>, fit: FitMode) {
        *self.0.lock().await = Some(TftMemoryState::Image { bytes, fit });
    }

    pub async fn forget(&self) {
        *self.0.lock().await = None;
    }

    /// Re-upload whatever was last remembered, if anything. Used by
    /// commands the firmware treats as TFT-resetting (currently:
    /// `apply_lighting`, `apply_preset`).
    ///
    /// Returns `Ok(false)` when there's nothing remembered, `Ok(true)`
    /// when a re-upload completed. Errors propagate so the caller can
    /// surface them — though in practice it's better to log + ignore
    /// because the user's primary command already succeeded.
    pub async fn restore_after_side_effect(self: &Arc<Self>) -> Result<bool, AppError> {
        let state = self.0.lock().await.clone();
        let Some(state) = state else {
            return Ok(false);
        };
        // Decode + upload off-thread; this matches the spawn_blocking
        // boundaries the original `apply_tft_*` commands use.
        tokio::task::spawn_blocking(move || -> Result<(), AppError> {
            let anim = match state {
                TftMemoryState::Preset(id) => tft_presets::build(&id).ok_or_else(|| {
                    AppError::Protocol(format!(
                        "stored TFT preset id `{id}` no longer in catalogue"
                    ))
                })?,
                TftMemoryState::Image { bytes, fit } => {
                    tft_image::animation_from_bytes(&bytes, fit).map_err(AppError::from)?
                }
            };
            let tft = Connection::open_tft().map_err(AppError::from)?;
            tft.upload_tft_animation(&anim).map_err(AppError::from)?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::Protocol(format!("join restore: {e}")))??;
        tracing::info!("TFT re-applied after side-effecting command");
        Ok(true)
    }
}
