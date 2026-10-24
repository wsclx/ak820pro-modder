//! Single gateway for everything that writes to the TFT.
//!
//! Uploads come from four places: the preset picker, the image upload,
//! the Now-Playing loop and the `TftMemory` restore. Each opens its own
//! HID handle on the TFT interface (`0xFF67`, or `0xFF68` on family B), so without a shared lock two
//! uploads can interleave their 4096-byte chunks on the wire and the
//! firmware stores a mix of both. Everything TFT-related goes through
//! [`TFT_LOCK`] here.

use std::sync::Arc;

use ak820_protocol::commands::tft::{TftAnimation, TftUploadReport};
use ak820_protocol::Connection;
use tokio::sync::{Mutex, MutexGuard};

use crate::AppError;

/// Serialises all TFT traffic, including `SET_TFT_BUILT_IN_INDEX` on the
/// control interface: switching the display slot mid-upload is just as
/// bad as two uploads racing.
static TFT_LOCK: Mutex<()> = Mutex::const_new(());

/// Hold this while sending a TFT-affecting command that isn't an upload.
pub async fn lock() -> MutexGuard<'static, ()> {
    TFT_LOCK.lock().await
}

/// Upload `anim` on a fresh TFT handle, one upload at a time. The HID work
/// runs on the blocking pool; with ack-paced chunks a GIF upload can take
/// seconds. Family-B boards are only written when `allow_legacy` carries
/// the user's confirmation (see `Connection::upload_tft_animation_confirmed`).
pub async fn upload(
    anim: Arc<TftAnimation>,
    allow_legacy: bool,
) -> Result<TftUploadReport, AppError> {
    let _guard = lock().await;
    tokio::task::spawn_blocking(move || -> Result<TftUploadReport, AppError> {
        let tft = Connection::open_tft()?;
        let slot = allow_legacy.then_some(ak820_protocol::legacy_tft::DEFAULT_SLOT);
        Ok(tft.upload_tft_animation_confirmed(&anim, slot)?)
    })
    .await
    .map_err(|e| AppError::Protocol(format!("join TFT upload: {e}")))?
}
