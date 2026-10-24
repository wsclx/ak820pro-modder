//! TFT transport for the FF13/FF68 feature-report firmware.
//!
//! This is deliberately separate from the AA/55 web-driver protocol. The
//! 04 72 01 image command replaces the stored animation before transfer, so
//! callers must explicitly opt in. No bootloader or firmware commands exist
//! here. See docs/MACOS-800A.md for sources and hardware verification status.

use crate::commands::tft::{TftAnimation, FRAME_HEADER_BYTES};
use crate::device::{DeviceInfo, DeviceProtocol};
use crate::{Error, Result};
use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const WORKER_ARG: &str = "--ak820-legacy-tft-worker";
const CHUNK_BYTES: usize = 4096;
const MAX_LEGACY_FRAMES: usize = 30;
/// IMAGE_CFG byte 2 is the one-based destination slot (vendor ANSI driver
/// 1.0.0.5: LCDViewList::GetCurSel()+1, layout gif_count=1). Slot 3 was
/// acknowledged on an ANSI 800A but showed the image only briefly among
/// other animation content; slot 1 kept it. From vespovios/ak820pro-modder@3264608.
const IMAGE_SLOT: u8 = 1;

#[derive(Serialize, Deserialize)]
struct Upload {
    target: DeviceInfo,
    animation: TftAnimation,
}

fn failure(message: impl Into<String>) -> Error {
    Error::UnexpectedResponse(message.into())
}

/// Construct the older format: all frame delays in 2 ms units, followed by
/// RGB565 LE pixels and FF padding. It has no AA/50 per-chunk header.
pub fn encode_animation(animation: &TftAnimation) -> Result<Vec<u8>> {
    if animation.frames.len() > MAX_LEGACY_FRAMES {
        return Err(Error::OutOfRange {
            field: "legacy TFT frame count",
            value: animation.frames.len() as i64,
            max: MAX_LEGACY_FRAMES as i64,
        });
    }
    // Reuse size and empty-animation validation, not the modern timing header.
    let mut data = animation.encode()?;
    data[..FRAME_HEADER_BYTES].fill(0xFF);
    data[0] = animation.frames.len() as u8;
    if animation.frames.len() == 1 {
        data[1] = 0;
    } else {
        for (i, frame) in animation.frames.iter().enumerate() {
            data[i + 1] = (frame.delay_ms / 2).clamp(1, 255) as u8;
        }
    }
    data.resize(data.len().div_ceil(CHUNK_BYTES) * CHUNK_BYTES, 0xFF);
    Ok(data)
}

/// Supervise the synchronous macOS HID writer in a separate process. A
/// blocked IOHIDDeviceSetReport must not freeze the app or keep a hidden
/// upload running after the timeout. There are no retries or reset commands.
/// Returns the number of 4096-byte blocks, all of them acknowledged.
pub(crate) fn upload_in_worker(target: &DeviceInfo, animation: &TftAnimation) -> Result<usize> {
    let chunks = encode_animation(animation)?.len() / CHUNK_BYTES;
    let request = serde_json::to_vec(&Upload {
        target: target.clone(),
        animation: animation.clone(),
    })
    .map_err(|e| failure(format!("encode worker request: {e}")))?;
    let mut child = Command::new(std::env::current_exe().map_err(|e| failure(e.to_string()))?)
        .arg(WORKER_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failure(format!("start TFT worker: {e}")))?;
    let sent = child.stdin.take().unwrap().write_all(&request);
    if let Err(e) = sent {
        let _ = child.kill();
        let _ = child.wait();
        return Err(failure(format!("send TFT worker request: {e}")));
    }
    let deadline = Instant::now() + Duration::from_secs((10 + chunks as u64 * 2).min(180));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut detail = String::new();
                if let Some(stderr) = child.stderr.take() {
                    let _ = stderr.take(4096).read_to_string(&mut detail);
                }
                return if status.success() {
                    Ok(chunks)
                } else {
                    Err(failure(format!("Legacy TFT upload stopped: {}. The stored animation may need another upload.", detail.trim())))
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure(format!("TFT worker timed out or failed ({result:?}); transfer stopped. The previous animation may need another upload.")));
            }
        }
    }
}

/// Both shipped executables call this before starting their normal runtime.
/// The worker opens only the exact previously selected device, then exits.
pub fn worker_entry() {
    if std::env::args().nth(1).as_deref() != Some(WORKER_ARG) {
        return;
    }
    let result = (|| -> Result<()> {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(8 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| failure(e.to_string()))?;
        let request: Upload = serde_json::from_slice(&bytes).map_err(|e| failure(e.to_string()))?;
        let payload = encode_animation(&request.animation)?;
        let candidates = crate::enumerate()?;
        let target = candidates
            .iter()
            .find(|d| {
                d.path == request.target.path
                    && d.pid == request.target.pid
                    && d.protocol == DeviceProtocol::LegacyFeature
                    && d.is_tft
            })
            .ok_or_else(|| failure("Selected legacy TFT device disappeared"))?;
        let controls: Vec<_> = candidates
            .iter()
            .filter(|d| {
                d.is_control
                    && d.protocol == DeviceProtocol::LegacyFeature
                    && d.pid == target.pid
                    && d.product == target.product
                    && d.serial == target.serial
            })
            .collect();
        if controls.len() != 1 {
            return Err(failure(
                "Cannot uniquely identify matching legacy control interface",
            ));
        }
        let api = HidApi::new()?;
        #[cfg(target_os = "macos")]
        api.set_open_exclusive(false);
        let open = |info: &DeviceInfo| -> Result<HidDevice> {
            let path =
                std::ffi::CString::new(info.path.as_str()).map_err(|e| failure(e.to_string()))?;
            Ok(api.open_path(&path)?)
        };
        let control = open(controls[0])?;
        let data = open(target)?;
        // Match complete descriptors observed on the user's keyboard before
        // issuing 04 72. Similar Sonix devices must never reach this path.
        const CONTROL_DESC: &[u8] = &[
            0x06, 0x13, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08,
            0x95, 0x40, 0x09, 0x02, 0x81, 0x02, 0x09, 0x03, 0x91, 0x02, 0x0a, 0x00, 0xff, 0xb1,
            0x02, 0xc0,
        ];
        const DATA_DESC: &[u8] = &[
            0x06, 0x68, 0xff, 0x09, 0x61, 0xa1, 0x01, 0x09, 0x62, 0x15, 0x00, 0x26, 0xff, 0x00,
            0x95, 0x40, 0x75, 0x08, 0x81, 0x02, 0x09, 0x63, 0x15, 0x00, 0x26, 0xff, 0x00, 0x96,
            0x00, 0x10, 0x75, 0x08, 0x91, 0x02, 0xc0,
        ];
        for (dev, expected) in [(&control, CONTROL_DESC), (&data, DATA_DESC)] {
            let mut desc = [0u8; 512];
            let n = dev.get_report_descriptor(&mut desc)?;
            if &desc[..n] != expected {
                return Err(failure("Legacy HID descriptor differs from the verified hardware layout; no write sent"));
            }
        }
        let _lock = crate::legacy::TransactionLock::acquire(target.pid)?;
        let chunks = payload.len() / CHUNK_BYTES;
        feature(&control, 0x18, 0, 0)?;
        feature(&control, 0x72, IMAGE_SLOT, chunks as u16)?;
        for (i, chunk) in payload.chunks_exact(CHUNK_BYTES).enumerate() {
            let mut report = [0u8; CHUNK_BYTES + 1];
            report[1..].copy_from_slice(chunk);
            if data.write(&report)? != report.len() {
                return Err(failure(format!("Short TFT write at chunk {}", i + 1)));
            }
            let mut ack = [0u8; 64];
            if data.read_timeout(&mut ack, 1000)? == 0 {
                return Err(failure(format!(
                    "No acknowledgement for TFT chunk {}/{}; SAVE was not sent",
                    i + 1,
                    chunks
                )));
            }
        }
        feature(&control, 0x02, 0, 0)?;
        std::thread::sleep(Duration::from_millis(100));
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

fn feature(device: &HidDevice, command: u8, subcommand: u8, value: u16) -> Result<()> {
    // ID 0 is separate from the 04 first payload byte: 65 bytes to hidapi.
    let mut report = [0u8; 65];
    report[1..].copy_from_slice(&crate::legacy::control_packet(command, subcommand, value));
    device.send_feature_report(&report)?;
    std::thread::sleep(Duration::from_millis(50));
    let mut response = [0u8; 65];
    let n = device.get_feature_report(&mut response)?;
    if n != 65 || response[1..3] != [0x04, command] {
        return Err(failure(format!(
            "Unexpected feature handshake for command 0x{command:02x}; transfer stopped"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tft::{TftFrame, FRAME_BYTES};

    #[test]
    fn legacy_stream_keeps_all_delays_and_pixels_without_web_headers() {
        let animation = TftAnimation {
            frames: vec![
                TftFrame {
                    pixels: vec![0x12; FRAME_BYTES],
                    delay_ms: 100,
                },
                TftFrame {
                    pixels: vec![0x34; FRAME_BYTES],
                    delay_ms: 200,
                },
            ],
        };
        let data = encode_animation(&animation).unwrap();
        assert_eq!(&data[..4], &[2, 50, 100, 0xff]);
        assert_eq!(data.len(), 17 * CHUNK_BYTES);
        assert!(data[256..256 + FRAME_BYTES].iter().all(|&b| b == 0x12));
        assert!(data[256 + FRAME_BYTES..256 + 2 * FRAME_BYTES]
            .iter()
            .all(|&b| b == 0x34));
        assert!(data[256 + 2 * FRAME_BYTES..].iter().all(|&b| b == 0xff));
    }

    #[test]
    fn image_config_targets_the_first_custom_slot_with_the_block_count() {
        for total in [9u16, 17, 257] {
            let mut expected = [0; 64];
            expected[..3].copy_from_slice(&[4, 0x72, 1]);
            expected[8..10].copy_from_slice(&total.to_le_bytes());
            assert_eq!(
                crate::legacy::control_packet(0x72, IMAGE_SLOT, total),
                expected
            );
        }
    }

    #[test]
    fn invalid_animation_is_rejected_before_worker_starts() {
        assert!(encode_animation(&TftAnimation::default()).is_err());
        let frame = TftFrame {
            pixels: vec![0; FRAME_BYTES],
            delay_ms: 100,
        };
        assert!(encode_animation(&TftAnimation {
            frames: vec![frame; 31]
        })
        .is_err());
    }
}
