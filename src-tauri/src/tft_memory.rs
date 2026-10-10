//! Remember the last-applied TFT content so we can re-apply it after
//! commands that the firmware treats as side-effecting.
//!
//! ## Why this exists
//!
//! Mario reported that applying a Lighting effect resets the TFT panel
//! back to its built-in factory animation, wiping whatever user content
//! we had uploaded. The official AJAZZ web driver sends the exact same
//! 16-byte `SET_LED_EFFECT` payload we do, so the trigger looked
//! firmware-side. The next-best move was to remember what the user last
//! put on the TFT and re-upload it after every command suspected of
//! wiping the panel (`apply_lighting`, `set_keymap`, `set_fn_keymap`,
//! `set_macros`, `set_custom_led`, `set_game_mode`, `apply_preset`).
//!
//! **Open question (Sprint 1, October 2026):** uploads used to be sent
//! without flow control. If the wipe was a side effect of half-committed
//! uploads, the ack-paced upload in `ak820-protocol` makes this module
//! unnecessary. Re-test on hardware and delete it if the panel survives
//! a Lighting change on its own.
//!
//! ## What's tracked
//!
//! The already-built animation, so a restore never re-decodes a GIF or
//! regenerates a preset. `None` = nothing to restore (the user hit
//! Factory Default, started Now-Playing, or never touched the TFT).
//!
//! ## Debounced restore
//!
//! Callers only *schedule* a restore. It runs [`RESTORE_DEBOUNCE`] after
//! the last side-effecting command, so painting ten keys in a row costs
//! one TFT upload instead of ten, and the caller's command returns
//! without waiting for a multi-second GIF upload.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ak820_protocol::commands::tft::TftAnimation;
use tokio::sync::Mutex;

use crate::tft_io;

/// Quiet period after the last side-effecting command before the TFT
/// content is re-uploaded.
const RESTORE_DEBOUNCE: Duration = Duration::from_millis(750);

struct Remembered {
    /// Human-readable origin for logs ("preset plasma", "image cat.gif").
    label: String,
    anim: Arc<TftAnimation>,
}

#[derive(Default)]
pub struct TftMemory {
    state: Mutex<Option<Remembered>>,
    /// Bumped by every remember / forget / schedule. A pending restore only
    /// fires if no newer event happened during its debounce window.
    generation: AtomicU64,
}

impl TftMemory {
    pub async fn remember(&self, label: String, anim: Arc<TftAnimation>) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.state.lock().await = Some(Remembered { label, anim });
    }

    pub async fn forget(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.state.lock().await = None;
    }

    /// Re-upload the remembered content once side-effecting commands have
    /// been quiet for [`RESTORE_DEBOUNCE`]. Free no-op when nothing is
    /// remembered. Failures are logged, not surfaced: the command that
    /// triggered the restore already succeeded.
    pub fn schedule_restore(self: &Arc<Self>) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let memory = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(RESTORE_DEBOUNCE).await;
            if memory.generation.load(Ordering::SeqCst) != generation {
                return; // superseded by a newer command or upload
            }
            let remembered = memory
                .state
                .lock()
                .await
                .as_ref()
                .map(|r| (r.label.clone(), r.anim.clone()));
            let Some((label, anim)) = remembered else {
                return;
            };
            match tft_io::upload(anim).await {
                Ok(report) => {
                    tracing::info!(%label, ?report, "TFT re-applied after side-effecting command")
                }
                Err(e) => tracing::warn!(%label, "TFT restore failed: {e:?}"),
            }
        });
    }
}
