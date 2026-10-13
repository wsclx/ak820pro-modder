use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace};

use crate::commands::keymap::{Keymap, KEYMAP_BYTES};
use crate::commands::lighting::{self, LightingConfig};
use crate::commands::macros::{
    self as macro_cmds, IndexEntry, Macro, MACRO_DATA_ADDR, MACRO_INDEX_BYTES,
};
use crate::commands::per_key_rgb::{CustomLedMap, CUSTOM_LED_BYTES};
use crate::commands::system::{DeviceInfoReport, GameMode};
use crate::commands::tft::{build_tft_header, TftAnimation};
use crate::protocol::{
    build_frame, cmd, HEADER_LEN, MAGIC_INCOMING, PACKET_LEN, PAYLOAD_PER_PACKET, REPORT_ID,
};
use crate::{error::*, CONTROL_INTERFACE, PRODUCT_IDS, VENDOR_ID};

/// Default response timeout (ms) for non-streaming GET/SET commands.
const DEFAULT_TIMEOUT_MS: i32 = 500;

/// How many streaming per-key RGB frames elapse between full input-buffer
/// drains.
///
/// `set_custom_led` skips the per-chunk ACK so a 128-LED frame (10 HID
/// reports) can finish fast enough to exceed ~15 fps. The host still reads
/// the response of the **final chunk of every frame**. Every Nth frame it
/// also drains whatever else is sitting in the HID input buffer, so unread
/// intermediate ACKs cannot fill the firmware's IN endpoint or be mistaken
/// for the next command's reply.
///
/// `1` drains after every frame. `0` disables the periodic drain (the
/// final-chunk read still happens, and every read-expecting command drains
/// before it writes).
///
/// Override per connection with
/// [`Connection::set_led_stream_sync_every_n_frames`], or at open time with
/// the `AK820_LED_SYNC_EVERY_N_FRAMES` environment variable.
///
/// **Hardware-unverified.** Mario must confirm on a real AK820 Pro that
/// this cadence stays ahead of the firmware buffer.
pub const LED_STREAM_SYNC_EVERY_N_FRAMES: u32 = 4;

/// Optional pause between streaming LED chunks (not after the last chunk).
///
/// [`Duration::ZERO`] keeps a 128-LED frame write-bound. A few hundred
/// microseconds is the escape hatch when a board overruns its OUT buffer
/// even with the final-chunk ACK.
///
/// Override per connection with
/// [`Connection::set_led_stream_inter_chunk_delay`], or at open time with
/// `AK820_LED_CHUNK_DELAY_US` (integer microseconds).
pub const LED_STREAM_INTER_CHUNK_DELAY: Duration = Duration::ZERO;

/// Cap on a single input-buffer drain. One LED frame is 10 reports; 64
/// covers several undrained frames without letting a wedged device spin.
const DRAIN_MAX_REPORTS: usize = 64;

/// HID byte pipe used by [`Connection`].
///
/// Production opens a `hidapi::HidDevice`. Tests supply a mock that records
/// read and write calls so the streaming ACK policy can be checked without
/// hardware.
pub trait HidTransport {
    fn write_report(&self, data: &[u8]) -> Result<usize>;
    fn read_report_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<usize>;
}

impl HidTransport for HidDevice {
    fn write_report(&self, data: &[u8]) -> Result<usize> {
        Ok(self.write(data)?)
    }

    fn read_report_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<usize> {
        Ok(self.read_timeout(buf, timeout_ms)?)
    }
}

/// Whether a multi-chunk SET waits for a firmware response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SetAckMode {
    /// Read after every chunk. Keymap and macro writes use this — a dropped
    /// chunk there corrupts persistent state, and the payloads are not a
    /// realtime stream.
    EveryChunk,
    /// Write-only stream. Skip per-chunk reads; ACK the final chunk of the
    /// frame, and drain the input buffer every N frames. Per-key RGB uses
    /// this so a full 128-LED update can exceed ~15 fps.
    Streaming,
}

/// Runtime knobs for [`SetAckMode::Streaming`]. Atomics because every
/// protocol method takes `&self` (the Tauri shell shares one connection).
struct LedStreamTuning {
    sync_every_n_frames: AtomicU32,
    inter_chunk_delay_us: AtomicU64,
    /// Streaming frames observed on this connection. The first frame is 1.
    frames_sent: AtomicU32,
}

impl LedStreamTuning {
    #[cfg(test)]
    fn from_defaults() -> Self {
        Self {
            sync_every_n_frames: AtomicU32::new(LED_STREAM_SYNC_EVERY_N_FRAMES),
            inter_chunk_delay_us: AtomicU64::new(duration_as_micros(LED_STREAM_INTER_CHUNK_DELAY)),
            frames_sent: AtomicU32::new(0),
        }
    }

    fn from_env() -> Self {
        let sync = parse_env_u32(
            std::env::var("AK820_LED_SYNC_EVERY_N_FRAMES")
                .ok()
                .as_deref(),
            LED_STREAM_SYNC_EVERY_N_FRAMES,
        );
        let delay_us = parse_env_u64(
            std::env::var("AK820_LED_CHUNK_DELAY_US").ok().as_deref(),
            duration_as_micros(LED_STREAM_INTER_CHUNK_DELAY),
        );
        Self {
            sync_every_n_frames: AtomicU32::new(sync),
            inter_chunk_delay_us: AtomicU64::new(delay_us),
            frames_sent: AtomicU32::new(0),
        }
    }
}

fn duration_as_micros(delay: Duration) -> u64 {
    u64::try_from(delay.as_micros()).unwrap_or(u64::MAX)
}

fn parse_env_u32(raw: Option<&str>, default: u32) -> u32 {
    match raw {
        Some(value) => match value.trim().parse::<u32>() {
            Ok(parsed) => parsed,
            Err(_) => {
                tracing::warn!(value, default, "invalid AK820_LED_SYNC_EVERY_N_FRAMES");
                default
            }
        },
        None => default,
    }
}

fn parse_env_u64(raw: Option<&str>, default: u64) -> u64 {
    match raw {
        Some(value) => match value.trim().parse::<u64>() {
            Ok(parsed) => parsed,
            Err(_) => {
                tracing::warn!(value, default, "invalid AK820_LED_CHUNK_DELAY_US");
                default
            }
        },
        None => default,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub vid: u16,
    pub pid: u16,
    pub interface: i32,
    pub usage_page: u16,
    pub usage: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub path: String,
}

/// Summary of one HID interface's report-descriptor properties.
///
/// We use this to find the right interface for chunked TFT uploads: the
/// `SET_TFT_USER_ANIMATION` path needs an output-report payload of ~4096
/// bytes, which is wildly different from the 64-byte control interface.
/// Opening the wrong interface and writing big chunks ends with cryptic
/// "frame too long" / IOHIDDevice errors on macOS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceProbe {
    /// Per-interface DeviceInfo (path, usage_page, product, …).
    pub info: DeviceInfo,
    /// Largest output-report size observed in the descriptor, in bytes
    /// (excluding the report-ID prefix). `None` means we couldn't open the
    /// device or the descriptor was empty.
    pub max_output_report_bytes: Option<usize>,
    /// Raw report descriptor bytes for the curious / for future RE.
    pub raw_descriptor_hex: Option<String>,
}

/// Open every AK820 candidate, read its report descriptor, and return a
/// summary used by RE / interface-selection logic.
///
/// **Caveat**: this acquires an exclusive HID handle for each interface in
/// turn. Don't call it while the Tauri shell already holds a connection.
pub fn probe_interfaces() -> Result<Vec<InterfaceProbe>> {
    let api = HidApi::new()?;
    let infos = enumerate()?;
    let mut out = Vec::with_capacity(infos.len());
    for info in infos {
        let opened = api.open_path(&std::ffi::CString::new(info.path.clone()).unwrap());
        let mut probe = InterfaceProbe {
            info: info.clone(),
            max_output_report_bytes: None,
            raw_descriptor_hex: None,
        };
        if let Ok(dev) = opened {
            let mut buf = vec![0u8; 4096];
            if let Ok(n) = dev.get_report_descriptor(&mut buf) {
                let desc = &buf[..n];
                probe.raw_descriptor_hex = Some(hex::encode(desc));
                probe.max_output_report_bytes = parse_max_output_report_size(desc);
            }
        }
        out.push(probe);
    }
    Ok(out)
}

/// Minimal HID-descriptor walker: returns the largest payload size (in bytes)
/// of any Output (= 0x91) report defined in the descriptor.
///
/// HID descriptors are a stream of variable-length items. Each item starts
/// with a 1-byte tag that encodes (size, type, tag). We only care about the
/// **Global** items `Report Size` (0x75) and `Report Count` (0x95), plus the
/// **Main** item `Output` (0x91). Whenever an `Output` is emitted, the most
/// recent Report Size × Report Count gives that report's payload bit-width;
/// we round up to bytes and take the max across all such Output emissions.
///
/// Not a full parser — but good enough to identify the "big report"
/// interface used for TFT uploads on the AK820 Pro.
fn parse_max_output_report_size(desc: &[u8]) -> Option<usize> {
    let mut i = 0;
    let mut report_size_bits: u32 = 0;
    let mut report_count: u32 = 0;
    let mut max_bytes: usize = 0;
    while i < desc.len() {
        let head = desc[i];
        let size_code = head & 0x03;
        let size = match size_code {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 4,
            _ => 0,
        };
        let tag = head & 0xFC;
        if i + 1 + size > desc.len() {
            break;
        }
        let data: u32 = match size {
            0 => 0,
            1 => desc[i + 1] as u32,
            2 => (desc[i + 1] as u32) | ((desc[i + 2] as u32) << 8),
            4 => {
                (desc[i + 1] as u32)
                    | ((desc[i + 2] as u32) << 8)
                    | ((desc[i + 3] as u32) << 16)
                    | ((desc[i + 4] as u32) << 24)
            }
            _ => 0,
        };
        match tag {
            // Global: Report Size (bits per field)
            0x74 => report_size_bits = data,
            // Global: Report Count (number of fields)
            0x94 => report_count = data,
            // Main: Output
            0x90 => {
                let bits = report_size_bits.saturating_mul(report_count);
                let bytes = bits.div_ceil(8) as usize;
                if bytes > max_bytes {
                    max_bytes = bytes;
                }
            }
            _ => {}
        }
        i += 1 + size;
    }
    if max_bytes == 0 {
        None
    } else {
        Some(max_bytes)
    }
}

pub fn enumerate() -> Result<Vec<DeviceInfo>> {
    let api = HidApi::new()?;
    let mut found = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() != VENDOR_ID || !PRODUCT_IDS.contains(&d.product_id()) {
            continue;
        }
        let info = DeviceInfo {
            vid: d.vendor_id(),
            pid: d.product_id(),
            interface: d.interface_number(),
            usage_page: d.usage_page(),
            usage: d.usage(),
            manufacturer: d.manufacturer_string().map(str::to_owned),
            product: d.product_string().map(str::to_owned),
            serial: d.serial_number().map(str::to_owned),
            path: d.path().to_string_lossy().into_owned(),
        };
        debug!(?info, "candidate HID device");
        found.push(info);
    }
    info!(count = found.len(), "enumerated AK820 candidates");
    Ok(found)
}

pub struct Connection<D: HidTransport = HidDevice> {
    device: D,
    info: DeviceInfo,
    led_stream: LedStreamTuning,
}

impl Connection<HidDevice> {
    /// Borrow the underlying hidapi handle.
    pub fn raw(&self) -> &HidDevice {
        &self.device
    }

    /// Open the AK820 control endpoint. Selection precedence:
    ///   1. `AK820_IFACE=N` → first device with `interface_number == N`
    ///   2. `AK820_USAGE_PAGE=0xFFXX` → first device with that usage page
    ///   3. default: vendor-specific usage page `0xFF67`
    pub fn open_control() -> Result<Self> {
        let candidates = enumerate()?;

        let by_iface_env = std::env::var("AK820_IFACE")
            .ok()
            .and_then(|v| v.parse::<i32>().ok());
        let by_usage_env = std::env::var("AK820_USAGE_PAGE")
            .ok()
            .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok());

        // The official AJAZZ online driver filters collections by usage page —
        // `q(x)` in the source accepts [0xFF68, 0xFF80, 0xFF60, 0xFF00, 0xFF01, 0xFF1B].
        // On the AK820 Pro that's interface 2 (0xFF68), not interface 3 (0xFF67).
        const PREFERRED_USAGE_PAGES: &[u16] = &[0xFF68, 0xFF80, 0xFF60, 0xFF00, 0xFF01, 0xFF1B];

        let pick = if let Some(want) = by_iface_env {
            candidates.iter().find(|d| d.interface == want).cloned()
        } else if let Some(want) = by_usage_env {
            candidates.iter().find(|d| d.usage_page == want).cloned()
        } else {
            candidates
                .iter()
                .find(|d| PREFERRED_USAGE_PAGES.contains(&d.usage_page))
                .or_else(|| candidates.iter().find(|d| d.interface == CONTROL_INTERFACE))
                .cloned()
        };

        let control = pick.ok_or(Error::DeviceNotFound {
            vid: VENDOR_ID,
            interface: CONTROL_INTERFACE,
        })?;

        let api = HidApi::new()?;
        let device = api.open_path(&std::ffi::CString::new(control.path.clone()).unwrap())?;
        device.set_blocking_mode(true)?;
        info!(
            path = %control.path,
            interface = control.interface,
            usage_page = format!("0x{:04x}", control.usage_page),
            "opened control interface"
        );
        Ok(Self {
            device,
            info: control,
            led_stream: LedStreamTuning::from_env(),
        })
    }
}

impl<D: HidTransport> Connection<D> {
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// How often a streaming per-key RGB frame drains the HID input buffer.
    ///
    /// See [`LED_STREAM_SYNC_EVERY_N_FRAMES`]. `0` disables the periodic
    /// drain; the final chunk of each frame is still ACKed.
    pub fn set_led_stream_sync_every_n_frames(&self, every_n_frames: u32) {
        self.led_stream
            .sync_every_n_frames
            .store(every_n_frames, Ordering::Relaxed);
    }

    /// Pause inserted between streaming LED chunks. [`Duration::ZERO`]
    /// (the default) does not sleep.
    pub fn set_led_stream_inter_chunk_delay(&self, delay: Duration) {
        self.led_stream
            .inter_chunk_delay_us
            .store(duration_as_micros(delay), Ordering::Relaxed);
    }

    #[cfg(test)]
    fn from_transport(device: D) -> Self {
        Self {
            device,
            info: DeviceInfo {
                vid: 0,
                pid: 0,
                interface: 0,
                usage_page: 0,
                usage: 0,
                manufacturer: None,
                product: None,
                serial: None,
                path: String::new(),
            },
            led_stream: LedStreamTuning::from_defaults(),
        }
    }

    pub fn probe(&self) -> Result<ProbeReport> {
        Ok(ProbeReport {
            connected: true,
            interface: self.info.interface,
            product: self.info.product.clone(),
            firmware_version: None,
        })
    }

    /// Send one output report on this interface.
    fn write_output_report(&self, frame: &[u8; PACKET_LEN]) -> Result<()> {
        let mut buf = [0u8; PACKET_LEN + 1];
        buf[0] = REPORT_ID;
        buf[1..].copy_from_slice(frame);
        trace!(report_id = REPORT_ID, hex = %hex::encode(&buf[1..16]), "TX");
        self.device.write_report(&buf)?;
        Ok(())
    }

    /// Read one input report until either it matches `expected_cmd` or the
    /// timeout elapses. Returns the payload (bytes 8…end of one frame).
    fn read_response(&self, expected_cmd: u8, timeout_ms: i32) -> Result<Vec<u8>> {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
        let mut buf = [0u8; PACKET_LEN];
        loop {
            let remaining_ms = deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis()
                .min(i32::MAX as u128) as i32;
            if remaining_ms == 0 {
                return Err(Error::UnexpectedResponse(format!(
                    "timeout waiting for cmd 0x{:02x}",
                    expected_cmd,
                )));
            }
            let n = self.device.read_report_timeout(&mut buf, remaining_ms)?;
            if n == 0 {
                continue;
            }
            if buf[0] != MAGIC_INCOMING {
                trace!(magic = buf[0], "ignoring non-response packet");
                continue;
            }
            if buf[1] != expected_cmd {
                trace!(cmd = buf[1], "ignoring response for different cmd");
                continue;
            }
            return Ok(buf[HEADER_LEN..n.min(PACKET_LEN)].to_vec());
        }
    }

    /// Throw away unread input reports.
    ///
    /// Streaming LED writes leave intermediate chunk ACKs in the buffer on
    /// purpose. A later GET or SET that expects a matched response has to
    /// drop those first, or it will bind a stale reply to the new request
    /// and every pair after that shifts by one. Timeout is zero so an empty
    /// buffer costs one non-blocking read.
    fn drain_input(&self) -> Result<usize> {
        let mut buf = [0u8; PACKET_LEN];
        let mut drained = 0usize;
        for _ in 0..DRAIN_MAX_REPORTS {
            let n = self.device.read_report_timeout(&mut buf, 0)?;
            if n == 0 {
                break;
            }
            drained += 1;
        }
        if drained == DRAIN_MAX_REPORTS {
            tracing::warn!(
                drained,
                "HID input drain hit the per-call cap; more stale reports may remain"
            );
        } else if drained > 0 {
            trace!(drained, "drained stale HID input reports");
        }
        Ok(drained)
    }

    /// Run a single-chunk GET transaction: send request header, await response,
    /// return up to `content_size` payload bytes from the first response packet.
    fn get(&self, cmd_byte: u8, content_size: usize) -> Result<Vec<u8>> {
        if content_size > PAYLOAD_PER_PACKET {
            // Multi-chunk responses are needed for things like custom-LED (512 bytes)
            // — implement when we reach Phase 4/5. For now we only need single packets.
            return Err(Error::NotImplemented("multi-chunk GET not yet supported"));
        }
        let frame = build_frame(cmd_byte, content_size as u8, 0, &[], true);
        trace!(cmd = cmd_byte, content_size, "GET");
        self.drain_input()?;
        self.write_output_report(&frame)?;
        let payload = self.read_response(cmd_byte, DEFAULT_TIMEOUT_MS)?;
        Ok(payload.into_iter().take(content_size).collect())
    }

    /// Run a single-chunk SET transaction.
    fn set(&self, cmd_byte: u8, payload: &[u8]) -> Result<()> {
        if payload.len() > PAYLOAD_PER_PACKET {
            return Err(Error::NotImplemented("multi-chunk SET not yet supported"));
        }
        let frame = build_frame(cmd_byte, payload.len() as u8, 0, payload, true);
        trace!(cmd = cmd_byte, len = payload.len(), "SET");
        self.drain_input()?;
        self.write_output_report(&frame)?;
        // Set commands echo a response — drain it so it doesn't pollute the
        // next read. Ignore errors: some firmwares don't respond to SET ops.
        let _ = self.read_response(cmd_byte, DEFAULT_TIMEOUT_MS);
        Ok(())
    }

    /// Multi-chunk GET transaction. Sends one request per chunk and concatenates
    /// the responses. Mirrors the official driver's `C()` loop for content sizes
    /// that exceed a single packet's 56-byte payload (e.g. keymap reads at 512 B).
    fn get_many(&self, cmd_byte: u8, content_size: usize) -> Result<Vec<u8>> {
        let chunk = PAYLOAD_PER_PACKET;
        let num_chunks = content_size.div_ceil(chunk).max(1);
        let mut out = Vec::with_capacity(content_size);
        self.drain_input()?;
        for i in 0..num_chunks {
            let addr = (i * chunk) as u16;
            let remaining = content_size - i * chunk;
            let this_size = remaining.min(chunk);
            let is_last = i == num_chunks - 1;
            let frame = build_frame(cmd_byte, this_size as u8, addr, &[], is_last);
            trace!(cmd = cmd_byte, chunk = i, addr, this_size, "GET chunk");
            self.write_output_report(&frame)?;
            let payload = self.read_response(cmd_byte, DEFAULT_TIMEOUT_MS)?;
            let take = payload.len().min(this_size);
            out.extend_from_slice(&payload[..take]);
        }
        out.truncate(content_size);
        Ok(out)
    }

    /// Multi-chunk SET transaction. Slices the payload into ≤56-byte chunks
    /// and sends one request per chunk; the firmware acks each.
    fn set_many(&self, cmd_byte: u8, payload: &[u8]) -> Result<()> {
        self.set_many_at(cmd_byte, 0, payload, true, SetAckMode::EveryChunk)
    }

    /// Multi-chunk GET starting at an arbitrary base address.
    ///
    /// Matches the official driver's `C()` semantics: addr per chunk is
    /// `addr_base + i * PAYLOAD_PER_PACKET`. The `last_packet` flag stays
    /// `false` for non-final chunks of a GET (the firmware ignores it for
    /// reads, so this only matters for paired SET writes — see `set_many_at`).
    fn get_many_at(&self, cmd_byte: u8, addr_base: u16, content_size: usize) -> Result<Vec<u8>> {
        let chunk = PAYLOAD_PER_PACKET;
        let num_chunks = content_size.div_ceil(chunk).max(1);
        let mut out = Vec::with_capacity(content_size);
        self.drain_input()?;
        for i in 0..num_chunks {
            let addr = addr_base.wrapping_add((i * chunk) as u16);
            let remaining = content_size - i * chunk;
            let this_size = remaining.min(chunk);
            let is_last = i == num_chunks - 1;
            let frame = build_frame(cmd_byte, this_size as u8, addr, &[], is_last);
            trace!(cmd = cmd_byte, chunk = i, addr, this_size, "GET chunk");
            self.write_output_report(&frame)?;
            let payload = self.read_response(cmd_byte, DEFAULT_TIMEOUT_MS)?;
            let take = payload.len().min(this_size);
            out.extend_from_slice(&payload[..take]);
        }
        out.truncate(content_size);
        Ok(out)
    }

    /// Multi-chunk SET starting at an arbitrary base address with explicit
    /// commit semantics.
    ///
    /// - `addr_base`: bytes 3..4 of each frame increment from this base.
    /// - `commit_on_last`: if `true`, the final chunk sets the last-packet
    ///   flag (byte 6 = 1), telling the firmware to commit the transaction.
    ///   If `false`, *every* chunk has the flag cleared (used for two-phase
    ///   writes like `SET_MACRO` where the index page is written first with
    ///   `commit=false` and the data area follows with `commit=true`).
    /// - `ack`: [`SetAckMode::EveryChunk`] reads a response after every
    ///   chunk (keymap, macros). [`SetAckMode::Streaming`] is the per-key
    ///   RGB path: intermediate chunks are write-only so a full 128-LED
    ///   frame can exceed ~15 fps. The final chunk of each frame is still
    ///   read, and every [`LED_STREAM_SYNC_EVERY_N_FRAMES`] frames the
    ///   input buffer is drained. An optional inter-chunk delay
    ///   ([`LED_STREAM_INTER_CHUNK_DELAY`]) can be set when a board overruns
    ///   its OUT buffer.
    ///
    /// **Hardware-unverified** for the streaming mode. Mario must confirm
    /// on a real AK820 Pro.
    fn set_many_at(
        &self,
        cmd_byte: u8,
        addr_base: u16,
        payload: &[u8],
        commit_on_last: bool,
        ack: SetAckMode,
    ) -> Result<()> {
        let chunk = PAYLOAD_PER_PACKET;
        let num_chunks = payload.len().div_ceil(chunk).max(1);

        // Stale streaming ACKs would satisfy this loop's first read and
        // shift the rest of the transaction. Drop them before the first
        // write so each read pairs with the chunk we just sent.
        if ack == SetAckMode::EveryChunk {
            self.drain_input()?;
        }

        let periodic_sync = if ack == SetAckMode::Streaming {
            let every_n = self.led_stream.sync_every_n_frames.load(Ordering::Relaxed);
            let frame_number = self
                .led_stream
                .frames_sent
                .fetch_add(1, Ordering::Relaxed)
                .wrapping_add(1);
            every_n > 0 && frame_number % every_n == 0
        } else {
            false
        };
        let delay_us = if ack == SetAckMode::Streaming {
            self.led_stream.inter_chunk_delay_us.load(Ordering::Relaxed)
        } else {
            0
        };

        for i in 0..num_chunks {
            let addr = addr_base.wrapping_add((i * chunk) as u16);
            let start = i * chunk;
            let this_size = (payload.len() - start).min(chunk);
            let is_final_chunk = i + 1 == num_chunks;
            let last_flag = commit_on_last && is_final_chunk;
            let frame = build_frame(
                cmd_byte,
                this_size as u8,
                addr,
                &payload[start..start + this_size],
                last_flag,
            );
            trace!(
                cmd = cmd_byte,
                chunk = i,
                addr,
                this_size,
                last_flag,
                ?ack,
                "SET chunk"
            );
            self.write_output_report(&frame)?;
            match ack {
                SetAckMode::EveryChunk => {
                    // Some firmwares don't respond to SET. Swallow the
                    // timeout so a missing ACK doesn't fail the write.
                    let _ = self.read_response(cmd_byte, DEFAULT_TIMEOUT_MS);
                }
                SetAckMode::Streaming => {
                    if !is_final_chunk && delay_us > 0 {
                        std::thread::sleep(Duration::from_micros(delay_us));
                    }
                    if is_final_chunk {
                        // One ACK per frame is the overrun hedge: the
                        // firmware has produced a response, so it has
                        // consumed at least part of this frame. On every
                        // Nth frame, also sweep the intermediate ACKs we
                        // intentionally did not read.
                        let read_res = self.read_response(cmd_byte, DEFAULT_TIMEOUT_MS);
                        if periodic_sync {
                            self.drain_input()?;
                        }
                        read_res.map(|_| ())?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Read the full 128-slot keymap (one chunked GET_KEY transaction).
    pub fn get_keymap(&self) -> Result<Keymap> {
        let payload = self.get_many(cmd::GET_KEY, KEYMAP_BYTES)?;
        Ok(Keymap::decode(&payload))
    }

    /// Read the FN-layer keymap (same shape as the base layer).
    pub fn get_fn_keymap(&self) -> Result<Keymap> {
        let payload = self.get_many(cmd::GET_FN_KEY, KEYMAP_BYTES)?;
        Ok(Keymap::decode(&payload))
    }

    /// Read the firmware's **factory-default** base keymap — what the user
    /// would get after a full reset, without actually issuing the reset.
    /// Used by the UI's "Reset to factory" button: stage these slots into
    /// the draft so the user can review before saving.
    pub fn get_default_keymap(&self) -> Result<Keymap> {
        let payload = self.get_many(cmd::GET_DEFAULT_KEY_MATRIX, KEYMAP_BYTES)?;
        Ok(Keymap::decode(&payload))
    }

    /// Read the firmware's factory-default Fn-layer keymap.
    pub fn get_default_fn_keymap(&self) -> Result<Keymap> {
        let payload = self.get_many(cmd::GET_DEFAULT_FN_KEY_MATRIX, KEYMAP_BYTES)?;
        Ok(Keymap::decode(&payload))
    }

    /// Write the full 128-slot keymap.
    pub fn set_keymap(&self, km: &Keymap) -> Result<()> {
        let payload = km.encode();
        info!(bytes = payload.len(), "SET_KEY");
        self.set_many(cmd::SET_KEY, &payload)
    }

    /// Write the FN-layer keymap.
    pub fn set_fn_keymap(&self, km: &Keymap) -> Result<()> {
        let payload = km.encode();
        info!(bytes = payload.len(), "SET_FN_KEY");
        self.set_many(cmd::SET_FN_KEY, &payload)
    }

    /// Apply a complete lighting configuration.
    pub fn set_lighting(&self, cfg: &LightingConfig) -> Result<()> {
        let payload = lighting::led_effect_payload(cfg);
        let (r, g, b) = cfg.rgb();
        info!(
            mode = cfg.mode.name(),
            r, g, b,
            color_mode = cfg.color_mode,
            brightness = cfg.brightness,
            speed = cfg.speed,
            direction = ?cfg.direction,
            "SET_LED_EFFECT"
        );
        self.set(cmd::SET_LED_EFFECT, &payload)
    }

    /// Read every defined macro from the device.
    ///
    /// Walks the 400-byte index page, then for each non-empty slot pulls the
    /// 4-byte header and the per-action stream. Returns an `actions`-free
    /// macro for every slot that holds zero events on the device — callers
    /// can filter on `actions.is_empty()` if they want the strict list.
    pub fn get_macros(&self) -> Result<Vec<Macro>> {
        let index = self.get_many_at(cmd::GET_MACRO, 0, MACRO_INDEX_BYTES)?;
        let entries = macro_cmds::parse_index(&index);
        info!(slots_used = entries.len(), "GET_MACRO index");

        let mut macros = Vec::with_capacity(entries.len());
        for IndexEntry { macro_id, addr } in entries {
            let header = self.get_many_at(cmd::GET_MACRO, addr as u16, 4)?;
            if header.len() < 4 {
                continue;
            }
            let action_count = macro_cmds::parse_block_header(&header);
            let actions = if action_count > 0 {
                let bytes = self.get_many_at(
                    cmd::GET_MACRO,
                    addr.wrapping_add(4) as u16,
                    action_count * 4,
                )?;
                macro_cmds::parse_actions(&bytes, action_count)
            } else {
                Vec::new()
            };
            macros.push(Macro {
                macro_id,
                name: None,
                actions,
            });
        }
        Ok(macros)
    }

    /// Write a complete macro list to the device.
    ///
    /// Two-phase transaction: first the 400-byte index (no commit), then the
    /// concatenated data area at `addr = MACRO_DATA_ADDR` with the
    /// `last_packet` flag set on its final chunk.
    ///
    /// NB. To erase all macros, pass an empty slice — but note that the
    /// firmware needs at least one committed write for the change to stick;
    /// the no-data branch matches the AJAZZ driver verbatim and silently
    /// no-ops if there's nothing to write. For an "erase everything" path
    /// use `SET_FACTORY_RESET` with `MACRO_RESET = 4` (future work).
    pub fn set_macros(&self, macros: &[Macro]) -> Result<()> {
        let (index, data) = macro_cmds::encode_macros(macros)?;
        info!(
            slots = macros.iter().filter(|m| !m.actions.is_empty()).count(),
            data_bytes = data.len(),
            "SET_MACRO"
        );

        // Phase 1: write index (no commit yet).
        self.set_many_at(cmd::SET_MACRO, 0, &index, false, SetAckMode::EveryChunk)?;

        // Phase 2: write data area (commits the transaction).
        if !data.is_empty() {
            self.set_many_at(
                cmd::SET_MACRO,
                MACRO_DATA_ADDR,
                &data,
                true,
                SetAckMode::EveryChunk,
            )?;
        }
        Ok(())
    }

    /// Switch which TFT animation the display plays back. The keyboard has
    /// several built-in animations plus one user slot. Calling this on the
    /// control endpoint (cmd 81) before `upload_tft_animation` is what makes
    /// the upload actually visible — otherwise the device keeps cycling
    /// through the previous built-in slot while our buffer sits unused.
    ///
    /// Valid index range is `0..deviceInfo.builtInCount` (built-ins) plus a
    /// vendor-specific value for the user slot. Empirically: try `0` first;
    /// if that selects a built-in animation, retry with values up to ~10
    /// until one shows your uploaded user frames. The AJAZZ web driver
    /// stores the active index in `_.value` (per-device runtime state) and
    /// passes whatever the UI's slot selector returned.
    pub fn set_tft_built_in_index(&self, index: u8) -> Result<()> {
        info!(index, "SET_TFT_BUILT_IN_INDEX");
        self.set(cmd::SET_TFT_BUILT_IN_INDEX, &[index])
    }
}

impl Connection<HidDevice> {
    /// Open the **TFT upload** interface — a different HID endpoint than the
    /// one returned by `open_control()`. The TFT path uses 4096-byte payload
    /// chunks (vs 56-byte chunks elsewhere), so it has its own HID interface
    /// on the device (`usage_page = 0xFF67` on the AK820 Pro, confirmed by
    /// inspecting the report descriptor with `probe_interfaces()` — the
    /// `0xFF67` collection advertises a 4104-byte output report).
    ///
    /// Selection precedence: `AK820_TFT_USAGE_PAGE` env override, then the
    /// fixed default `0xFF67`. Callers can hold both `open_control()` and
    /// `open_tft()` connections concurrently — they're separate HID handles.
    pub fn open_tft() -> Result<Self> {
        let candidates = enumerate()?;
        let want_usage_page = std::env::var("AK820_TFT_USAGE_PAGE")
            .ok()
            .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0xFF67);
        let pick = candidates
            .iter()
            .find(|d| d.usage_page == want_usage_page)
            .cloned()
            .ok_or(Error::DeviceNotFound {
                vid: VENDOR_ID,
                interface: -1,
            })?;
        let api = HidApi::new()?;
        let device = api.open_path(&std::ffi::CString::new(pick.path.clone()).unwrap())?;
        device.set_blocking_mode(true)?;
        info!(
            path = %pick.path,
            interface = pick.interface,
            usage_page = format!("0x{:04x}", pick.usage_page),
            "opened TFT upload interface"
        );
        Ok(Self {
            device,
            info: pick,
            led_stream: LedStreamTuning::from_env(),
        })
    }
}

impl<D: HidTransport> Connection<D> {
    /// Upload a TFT animation as a single chunked transaction. Re-uses our
    /// existing `TftAnimation::encode()` for the 256-B-header + RGB565 stream
    /// and applies the bespoke 8-B per-chunk header — both decoded from the
    /// AJAZZ online driver.
    ///
    /// The chunk payload size is read from the device's HID descriptor (the
    /// `Items.reportCount` field in the JS source). For the AK820 Pro this
    /// is 4104 bytes per output report → 4096 bytes of payload after the
    /// 8-byte custom header.
    ///
    /// Must be called on a connection opened via `open_tft()` — the control
    /// interface's 64-byte report can't carry these chunks.
    pub fn upload_tft_animation(&self, anim: &TftAnimation) -> Result<()> {
        // Sanity-check the interface — if we're on the wrong one the
        // hidapi write will quietly fail with a frame-too-long error on macOS.
        if self.info.usage_page == 0xFF68 {
            return Err(Error::UnexpectedResponse(
                "upload_tft_animation called on the 0xFF68 control interface; \
                 use Connection::open_tft() to get the 0xFF67 4 KB-report interface"
                    .into(),
            ));
        }

        let payload = anim.encode()?;
        const HEADER_LEN: usize = 8;
        const TFT_REPORT_LEN: usize = 4104;
        const TFT_PAYLOAD_LEN: usize = TFT_REPORT_LEN - HEADER_LEN; // 4096

        let total_chunks = payload.len().div_ceil(TFT_PAYLOAD_LEN).max(1);
        if total_chunks > u16::MAX as usize {
            return Err(Error::OutOfRange {
                field: "tft chunk count",
                value: total_chunks as i64,
                max: u16::MAX as i64,
            });
        }

        info!(
            bytes = payload.len(),
            chunks = total_chunks,
            "SET_TFT_USER_ANIMATION upload"
        );

        // Pre-allocated per-chunk buffer: 1 report-id byte + 8 header + 4096 payload.
        let mut report = vec![0u8; 1 + TFT_REPORT_LEN];
        for i in 0..total_chunks {
            let start = i * TFT_PAYLOAD_LEN;
            let end = (start + TFT_PAYLOAD_LEN).min(payload.len());
            let header =
                build_tft_header(cmd::SET_TFT_USER_ANIMATION, i as u16, total_chunks as u16);

            // Reset payload region to zero (header overwrites bytes 1..9).
            for b in &mut report[1..] {
                *b = 0;
            }
            report[0] = REPORT_ID;
            report[1..1 + HEADER_LEN].copy_from_slice(&header);
            let dst = &mut report[1 + HEADER_LEN..1 + HEADER_LEN + (end - start)];
            dst.copy_from_slice(&payload[start..end]);

            trace!(chunk = i, total = total_chunks, "TFT TX");
            self.device.write_report(&report)?;
        }
        Ok(())
    }

    /// Read the 128-LED per-key colour map.
    pub fn get_custom_led(&self) -> Result<CustomLedMap> {
        let payload = self.get_many_at(cmd::GET_CUSTOM_LED_DATA, 0, CUSTOM_LED_BYTES)?;
        Ok(CustomLedMap::decode(&payload))
    }

    /// Write the 128-LED per-key colour map. Caller is responsible for
    /// switching the active lighting effect to one that reads from this
    /// buffer (the standard 20 modes ignore it).
    ///
    /// This is the write-only streaming path: intermediate chunks do not
    /// wait for `read_response`, so a full frame can exceed ~15 fps. The
    /// final chunk of each frame is still ACKed, and every
    /// [`LED_STREAM_SYNC_EVERY_N_FRAMES`] frames the input buffer is
    /// drained. **Hardware-unverified** — Mario must confirm smoothness
    /// and that a later keymap / lighting / macro command still round-trips.
    pub fn set_custom_led(&self, map: &CustomLedMap) -> Result<()> {
        let payload = map.encode();
        info!(bytes = payload.len(), "SET_CUSTOM_LED_DATA");
        self.set_many_at(
            cmd::SET_CUSTOM_LED_DATA,
            0,
            &payload,
            true,
            SetAckMode::Streaming,
        )
    }

    /// Read the device-info struct (firmware, battery, profile, …).
    pub fn get_device_info(&self) -> Result<DeviceInfoReport> {
        let payload = self.get(cmd::GET_DEVICE_INFO, 48)?;
        Ok(DeviceInfoReport::parse(&payload))
    }

    /// Read the game-mode struct (sleep timer, key delay, report rate, …).
    pub fn get_game_mode(&self) -> Result<GameMode> {
        let payload = self.get(cmd::GET_GAME_MODE, 56)?;
        Ok(GameMode::parse(&payload))
    }

    /// Write the game-mode struct. Sends all 56 bytes — callers should
    /// usually `get_game_mode` first, mutate one field, then `set_game_mode`.
    pub fn set_game_mode(&self, gm: &GameMode) -> Result<()> {
        let payload = gm.serialize();
        info!(
            sleep_time = gm.sleep_time,
            game_mode = gm.game_mode,
            "SET_GAME_MODE"
        );
        self.set(cmd::SET_GAME_MODE, &payload)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeReport {
    pub connected: bool,
    pub interface: i32,
    pub product: Option<String>,
    pub firmware_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Instant;

    /// `read_response` passes the time still left on its deadline, so a
    /// blocking read is a small amount under [`DEFAULT_TIMEOUT_MS`] rather
    /// than exactly that constant.
    fn is_response_timeout(timeout_ms: i32) -> bool {
        timeout_ms > 0 && timeout_ms <= DEFAULT_TIMEOUT_MS
    }

    #[derive(Clone, Copy, Debug)]
    struct ReadCall {
        timeout_ms: i32,
        n: usize,
    }

    struct MockState {
        writes: Vec<Vec<u8>>,
        inbox: VecDeque<Vec<u8>>,
        reads: Vec<ReadCall>,
        auto_ack: bool,
        /// Byte written at the start of an auto-generated response payload.
        auto_marker: u8,
    }

    struct MockHid {
        state: Mutex<MockState>,
    }

    impl MockHid {
        fn auto_ack() -> Self {
            Self {
                state: Mutex::new(MockState {
                    writes: Vec::new(),
                    inbox: VecDeque::new(),
                    reads: Vec::new(),
                    auto_ack: true,
                    auto_marker: 0x42,
                }),
            }
        }

        fn push_inbox(&self, packet: Vec<u8>) {
            self.state.lock().unwrap().inbox.push_back(packet);
        }

        fn inbox_len(&self) -> usize {
            self.state.lock().unwrap().inbox.len()
        }

        fn writes(&self) -> Vec<Vec<u8>> {
            self.state.lock().unwrap().writes.clone()
        }

        fn reads(&self) -> Vec<ReadCall> {
            self.state.lock().unwrap().reads.clone()
        }
    }

    impl HidTransport for MockHid {
        fn write_report(&self, data: &[u8]) -> Result<usize> {
            let mut state = self.state.lock().unwrap();
            state.writes.push(data.to_vec());
            if state.auto_ack {
                let cmd = data.get(2).copied().unwrap_or(0);
                let marker = state.auto_marker;
                state.inbox.push_back(response_packet(cmd, marker));
            }
            Ok(data.len())
        }

        fn read_report_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<usize> {
            let mut state = self.state.lock().unwrap();
            if let Some(packet) = state.inbox.pop_front() {
                let n = packet.len().min(buf.len());
                buf[..n].copy_from_slice(&packet[..n]);
                state.reads.push(ReadCall { timeout_ms, n });
                Ok(n)
            } else if timeout_ms == 0 {
                state.reads.push(ReadCall { timeout_ms, n: 0 });
                Ok(0)
            } else {
                state.reads.push(ReadCall { timeout_ms, n: 0 });
                Err(Error::UnexpectedResponse(
                    "mock HID inbox empty on blocking read".into(),
                ))
            }
        }
    }

    fn response_packet(cmd_byte: u8, marker: u8) -> Vec<u8> {
        let mut packet = vec![0u8; PACKET_LEN];
        packet[0] = MAGIC_INCOMING;
        packet[1] = cmd_byte;
        packet[HEADER_LEN] = marker;
        packet
    }

    fn streaming_conn(sync_every: u32) -> Connection<MockHid> {
        let conn = Connection::from_transport(MockHid::auto_ack());
        conn.set_led_stream_sync_every_n_frames(sync_every);
        conn.set_led_stream_inter_chunk_delay(Duration::ZERO);
        conn
    }

    fn blocking_reads(reads: &[ReadCall]) -> Vec<ReadCall> {
        reads
            .iter()
            .copied()
            .filter(|call| call.timeout_ms > 0)
            .collect()
    }

    fn full_led_chunks() -> usize {
        CUSTOM_LED_BYTES.div_ceil(PAYLOAD_PER_PACKET)
    }

    #[test]
    fn full_led_frame_is_ten_chunks() {
        // 128 LEDs × 4 bytes / 56-byte payload. The streaming tests below
        // pin read/write counts against this so a framing change can't
        // silently shrink the transfer.
        assert_eq!(full_led_chunks(), 10);
    }

    #[test]
    fn streaming_led_write_skips_per_chunk_ack_and_reads_final_chunk() {
        let conn = streaming_conn(0);
        conn.set_custom_led(&CustomLedMap::default()).unwrap();

        let writes = conn.device.writes();
        let reads = conn.device.reads();
        assert_eq!(writes.len(), 10, "one output report per chunk");
        // Report layout: [report_id, magic, cmd, len, addr_lo, addr_hi, opt0, last_flag, ...]
        assert_eq!(writes[0][2], cmd::SET_CUSTOM_LED_DATA);
        assert_eq!(writes[0][7], 0, "intermediate chunk is not the commit");
        assert_eq!(writes[9][7], 1, "final chunk commits the frame");

        let blocking = blocking_reads(&reads);
        assert_eq!(
            blocking.len(),
            1,
            "only the final chunk of the frame waits for a response"
        );
        assert!(
            is_response_timeout(blocking[0].timeout_ms),
            "final-chunk read uses the normal response timeout, got {}",
            blocking[0].timeout_ms
        );
        assert!(blocking[0].n > 0);
        assert!(
            reads.iter().all(|call| call.timeout_ms > 0),
            "a non-sync frame must not drain (no zero-timeout reads)"
        );
        // 10 auto-ACKs were queued, one was consumed by the final-chunk read.
        assert_eq!(conn.device.inbox_len(), 9);
    }

    #[test]
    fn streaming_drains_input_every_n_frames() {
        let conn = streaming_conn(2);

        conn.set_custom_led(&CustomLedMap::default()).unwrap();
        assert_eq!(conn.device.writes().len(), 10);
        assert_eq!(blocking_reads(&conn.device.reads()).len(), 1);
        assert_eq!(
            conn.device.inbox_len(),
            9,
            "frame 1 leaves intermediate ACKs"
        );
        assert!(
            conn.device.reads().iter().all(|call| call.timeout_ms > 0),
            "frame 1 of every-2 is not a drain frame"
        );

        conn.set_custom_led(&CustomLedMap::default()).unwrap();
        assert_eq!(conn.device.writes().len(), 20);
        let reads = conn.device.reads();
        assert_eq!(blocking_reads(&reads).len(), 2, "each frame reads once");
        let drained = reads
            .iter()
            .filter(|call| call.timeout_ms == 0 && call.n > 0)
            .count();
        let drain_terminator = reads
            .iter()
            .filter(|call| call.timeout_ms == 0 && call.n == 0)
            .count();
        // Frame 2 starts with 9 leftover ACKs, queues 10 more, the final-chunk
        // read consumes 1, and the periodic drain takes the other 18.
        assert_eq!(drained, 18);
        assert_eq!(drain_terminator, 1);
        assert_eq!(conn.device.inbox_len(), 0);
    }

    #[test]
    fn every_chunk_set_still_reads_each_chunk() {
        let conn = streaming_conn(0);
        let payload = vec![0u8; PAYLOAD_PER_PACKET * 3];
        conn.set_many_at(cmd::SET_KEY, 0, &payload, true, SetAckMode::EveryChunk)
            .unwrap();

        assert_eq!(conn.device.writes().len(), 3);
        let reads = conn.device.reads();
        let blocking = blocking_reads(&reads);
        assert_eq!(blocking.len(), 3, "keymap-style writes ACK every chunk");
        assert!(
            blocking
                .iter()
                .all(|call| is_response_timeout(call.timeout_ms)),
            "per-chunk reads use the normal response timeout: {blocking:?}"
        );
        assert_eq!(
            reads.iter().filter(|call| call.timeout_ms == 0).count(),
            1,
            "one zero-timeout drain probe before the first chunk"
        );
        assert_eq!(conn.device.inbox_len(), 0);
    }

    #[test]
    fn read_expecting_command_drains_stale_same_cmd_response() {
        let conn = streaming_conn(0);
        conn.device
            .push_inbox(response_packet(cmd::GET_CUSTOM_LED_DATA, 0xEE));

        let map = conn.get_custom_led().unwrap();
        assert_eq!(
            map.leds[0].led_id, 0x42,
            "the GET payload must be the response queued by the write, not the stale ACK"
        );
        assert_ne!(map.leds[0].led_id, 0xEE);

        let reads = conn.device.reads();
        let first_blocking = reads.iter().position(|call| call.timeout_ms > 0).unwrap();
        assert!(
            reads[..first_blocking]
                .iter()
                .any(|call| call.timeout_ms == 0 && call.n > 0),
            "stale report is consumed by the pre-command drain"
        );
    }

    #[test]
    fn streaming_inter_chunk_delay_sleeps_between_chunks_only() {
        let conn = streaming_conn(0);
        conn.set_led_stream_inter_chunk_delay(Duration::from_millis(20));
        let started = Instant::now();
        conn.set_many_at(
            cmd::SET_CUSTOM_LED_DATA,
            0,
            &[0u8; PAYLOAD_PER_PACKET * 2],
            true,
            SetAckMode::Streaming,
        )
        .unwrap();
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(15),
            "two chunks insert one inter-chunk delay, elapsed={elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(250),
            "delay must not turn into a per-chunk response timeout, elapsed={elapsed:?}"
        );
        assert_eq!(conn.device.writes().len(), 2);
        assert_eq!(blocking_reads(&conn.device.reads()).len(), 1);
    }

    #[test]
    fn env_parsers_accept_integers_and_fall_back() {
        assert_eq!(parse_env_u32(Some("3"), 4), 3);
        assert_eq!(parse_env_u32(Some(" 8 "), 4), 8);
        assert_eq!(parse_env_u32(Some("nope"), 4), 4);
        assert_eq!(parse_env_u32(None, 4), 4);
        assert_eq!(parse_env_u64(Some("250"), 0), 250);
        assert_eq!(parse_env_u64(Some(""), 7), 7);
        assert_eq!(parse_env_u64(None, 7), 7);
    }
}
