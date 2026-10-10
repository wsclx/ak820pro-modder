use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace};

use crate::commands::clock::TftDateTime;
use crate::commands::keymap::{Keymap, KEYMAP_BYTES};
use crate::commands::lighting::{self, LightingConfig};
use crate::commands::macros::{
    self as macro_cmds, IndexEntry, Macro, MACRO_DATA_ADDR, MACRO_INDEX_BYTES,
};
use crate::commands::per_key_rgb::{CustomLedMap, CUSTOM_LED_BYTES};
use crate::commands::system::{DeviceInfoReport, GameMode};
use crate::commands::tft::{build_tft_header, TftAnimation, TftUploadReport};
use crate::protocol::{
    build_frame, cmd, HEADER_LEN, MAGIC_INCOMING, PACKET_LEN, PAYLOAD_PER_PACKET, REPORT_ID,
};
use crate::{error::*, CONTROL_INTERFACE, PRODUCT_IDS, VENDOR_ID};

/// Default response timeout (ms) for non-streaming GET/SET commands.
const DEFAULT_TIMEOUT_MS: i32 = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub vid: u16,
    pub pid: u16,
    pub interface: i32,
    pub usage_page: u16,
    pub usage: u16,
    /// USB `bcdDevice`. On the AK820 Pro this tracks the firmware
    /// revision (e.g. `0x0107` for 1.07, `0x0114` for 1.14), which lets
    /// bug reports identify the firmware even when no control command
    /// gets a response.
    pub release_number: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub path: String,
}

/// Usage pages the official AJAZZ driver accepts for the control
/// collection (`q(x)` in the web-driver source), in priority order. On
/// the AK820 Pro that's interface 2 (`0xFF68`), not interface 3 (`0xFF67`).
pub const CONTROL_USAGE_PAGES: &[u16] = &[0xFF68, 0xFF80, 0xFF60, 0xFF00, 0xFF01, 0xFF1B];

/// Usage page of the feature-report interface that newer AK820 Pro
/// revisions (e.g. ANSI boards on firmware 1.14) expose instead of the
/// `0xFF67` TFT collection. Those boards don't answer the `0xAA`-framed
/// protocol this crate speaks.
const FEATURE_REPORT_USAGE_PAGE: u16 = 0xFF13;

/// Usage page of the TFT upload collection on the boards we support.
const TFT_USAGE_PAGE: u16 = 0xFF67;

/// Pick the control endpoint from an enumeration. Selection precedence:
///   1. `AK820_IFACE=N` → first device with `interface_number == N`
///   2. `AK820_USAGE_PAGE=0xFFXX` → first device with that usage page
///   3. the first usage page of [`CONTROL_USAGE_PAGES`] that any candidate
///      exposes, then [`CONTROL_INTERFACE`] as a last resort
///
/// Rule 3 walks the priority list, not the enumeration order: macOS
/// doesn't guarantee the order in which it reports a device's interfaces.
pub fn select_control(candidates: &[DeviceInfo]) -> Option<&DeviceInfo> {
    let by_iface_env = std::env::var("AK820_IFACE")
        .ok()
        .and_then(|v| v.parse::<i32>().ok());
    let by_usage_env = std::env::var("AK820_USAGE_PAGE")
        .ok()
        .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok());

    if let Some(want) = by_iface_env {
        return candidates.iter().find(|d| d.interface == want);
    }
    if let Some(want) = by_usage_env {
        return candidates.iter().find(|d| d.usage_page == want);
    }
    CONTROL_USAGE_PAGES
        .iter()
        .find_map(|page| candidates.iter().find(|d| d.usage_page == *page))
        .or_else(|| candidates.iter().find(|d| d.interface == CONTROL_INTERFACE))
}

/// `true` when the enumeration looks like a board revision that speaks
/// the feature-report protocol instead of ours: it exposes `0xFF13` but
/// no `0xFF67` TFT collection.
fn looks_like_feature_report_board(candidates: &[DeviceInfo]) -> bool {
    candidates
        .iter()
        .any(|d| d.usage_page == FEATURE_REPORT_USAGE_PAGE)
        && !candidates.iter().any(|d| d.usage_page == TFT_USAGE_PAGE)
}

/// Response commands accepted as a TFT chunk ack. The web driver's
/// `setTftUserAnimation` expects cmd 65 (`SET_LED_USER_ANIMATION`); cmd 80
/// is accepted too in case a firmware echoes the request command.
const TFT_ACK_CMDS: [u8; 2] = [cmd::SET_LED_USER_ANIMATION, cmd::SET_TFT_USER_ANIMATION];

/// How long the first chunk waits to find out whether acks arrive at all.
/// The first chunk likely triggers the slowest flash erase, so this is
/// generous; a firmware that never acks pays it once per upload.
const TFT_ACK_PROBE_MS: i32 = 1000;

/// Per-chunk ack timeout once acks are known to arrive. Generous because
/// each 4096-byte chunk is likely one SPI-flash sector erase + program.
const TFT_ACK_TIMEOUT_MS: i32 = 1500;

/// Inter-chunk delay when the firmware doesn't ack.
const TFT_DEFAULT_PACE_MS: i32 = 10;

/// `AK820_TFT_PACE_MS` overrides the fallback pacing, so hardware tests
/// can try different values without a rebuild.
fn tft_pace_ms() -> i32 {
    std::env::var("AK820_TFT_PACE_MS")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|ms| *ms >= 0)
        .unwrap_or(TFT_DEFAULT_PACE_MS)
}

/// Flow-control state of one TFT upload. The first chunk decides whether
/// this firmware acks chunks; one missing ack later degrades to pacing so
/// a silent firmware can't stall a 200-chunk GIF for minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TftFlow {
    Probe,
    Acked,
    Paced,
}

impl TftFlow {
    /// How long to wait for the ack of the chunk just written. When pacing,
    /// the pace delay itself is spent listening, so a late ack still
    /// counts and an early one lets the next chunk go out sooner.
    fn ack_wait_ms(self, pace_ms: i32) -> i32 {
        match self {
            Self::Probe => TFT_ACK_PROBE_MS,
            Self::Acked => TFT_ACK_TIMEOUT_MS,
            Self::Paced => pace_ms,
        }
    }

    fn next(self, acked: bool) -> Self {
        match (self, acked) {
            (Self::Probe | Self::Acked, true) => Self::Acked,
            _ => Self::Paced,
        }
    }
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
            release_number: d.release_number(),
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

pub struct Connection {
    device: HidDevice,
    info: DeviceInfo,
    /// Set when the enumeration matched [`looks_like_feature_report_board`].
    /// Only used to turn a bare response timeout into an actionable error.
    feature_report_board: bool,
}

impl Connection {
    /// Open the AK820 control endpoint. See [`select_control`] for the
    /// selection rules.
    pub fn open_control() -> Result<Self> {
        let candidates = enumerate()?;
        let control = select_control(&candidates)
            .cloned()
            .ok_or(Error::DeviceNotFound {
                vid: VENDOR_ID,
                interface: CONTROL_INTERFACE,
            })?;
        let feature_report_board = looks_like_feature_report_board(&candidates);

        let api = HidApi::new()?;
        let device = api.open_path(&std::ffi::CString::new(control.path.clone()).unwrap())?;
        device.set_blocking_mode(true)?;
        info!(
            path = %control.path,
            interface = control.interface,
            usage_page = format!("0x{:04x}", control.usage_page),
            release = format!("0x{:04x}", control.release_number),
            feature_report_board,
            "opened control interface"
        );
        Ok(Self {
            device,
            info: control,
            feature_report_board,
        })
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn raw(&self) -> &HidDevice {
        &self.device
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
        self.device.write(&buf)?;
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
                return Err(self.timeout_error(expected_cmd));
            }
            let n = self.device.read_timeout(&mut buf, remaining_ms)?;
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

    /// Build the error for a response that never arrived. Names the
    /// interface we talked to, so a bug report alone shows whether the
    /// wrong endpoint was picked, and flags board revisions that speak a
    /// different protocol.
    fn timeout_error(&self, expected_cmd: u8) -> Error {
        let mut msg = format!(
            "timeout waiting for cmd 0x{:02x} (interface {}, usage page 0x{:04x}, release 0x{:04x})",
            expected_cmd, self.info.interface, self.info.usage_page, self.info.release_number,
        );
        if self.feature_report_board {
            msg.push_str(
                ". This keyboard exposes the 0xFF13 feature-report interface used by newer \
                 AK820 Pro revisions (e.g. firmware 1.14), which speak a protocol this app \
                 doesn't support yet",
            );
        }
        Error::UnexpectedResponse(msg)
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
        self.set_many_at(cmd_byte, 0, payload, true)
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
    fn set_many_at(
        &self,
        cmd_byte: u8,
        addr_base: u16,
        payload: &[u8],
        commit_on_last: bool,
    ) -> Result<()> {
        let chunk = PAYLOAD_PER_PACKET;
        let num_chunks = payload.len().div_ceil(chunk).max(1);
        for i in 0..num_chunks {
            let addr = addr_base.wrapping_add((i * chunk) as u16);
            let start = i * chunk;
            let this_size = (payload.len() - start).min(chunk);
            let is_final_chunk = i == num_chunks - 1;
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
                "SET chunk"
            );
            self.write_output_report(&frame)?;
            let _ = self.read_response(cmd_byte, DEFAULT_TIMEOUT_MS);
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
        self.set_many_at(cmd::SET_MACRO, 0, &index, false)?;

        // Phase 2: write data area (commits the transaction).
        if !data.is_empty() {
            self.set_many_at(cmd::SET_MACRO, MACRO_DATA_ADDR, &data, true)?;
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

    /// Set the keyboard's RTC, which the stock TFT clock page renders.
    /// Web driver `setTftDateTime`: 10-byte payload via
    /// `SET_TEMPORARY_COMMAND_DATA` (cmd 52) on the control interface.
    pub fn set_tft_datetime(&self, datetime: &TftDateTime) -> Result<()> {
        let payload = datetime.encode()?;
        info!(?datetime, "SET_TFT_DATE_TIME");
        self.set(cmd::SET_TEMPORARY_COMMAND_DATA, &payload)
    }

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
            .unwrap_or(TFT_USAGE_PAGE);
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
            feature_report_board: false,
        })
    }

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
    ///
    /// **Flow control.** The web driver waits for a cmd-65 acknowledgement
    /// after every chunk (HANDOFF § 6.9h); writing chunks back-to-back
    /// while the firmware is still busy is the prime suspect for the
    /// horizontal noise lines in #2. We don't know yet whether that ack
    /// arrives on this HID handle, so the first chunk probes for it:
    /// if it arrives, every chunk waits for its ack; if not (or if one goes
    /// missing later), the remaining chunks are paced with a fixed delay
    /// (`AK820_TFT_PACE_MS`, default 10 ms). The returned report says
    /// which path the upload took.
    pub fn upload_tft_animation(&self, anim: &TftAnimation) -> Result<TftUploadReport> {
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

        let pace_ms = tft_pace_ms();
        info!(
            bytes = payload.len(),
            chunks = total_chunks,
            "SET_TFT_USER_ANIMATION upload"
        );

        let started = std::time::Instant::now();
        let mut flow = TftFlow::Probe;
        let mut acknowledged = 0usize;
        // Pre-allocated per-chunk buffer: 1 report-id byte + 8 header + 4096 payload.
        let mut report = vec![0u8; 1 + TFT_REPORT_LEN];
        for i in 0..total_chunks {
            let start = i * TFT_PAYLOAD_LEN;
            let end = (start + TFT_PAYLOAD_LEN).min(payload.len());
            let header =
                build_tft_header(cmd::SET_TFT_USER_ANIMATION, i as u16, total_chunks as u16);

            // Reset payload region to zero (header overwrites bytes 1..9).
            report[1..].fill(0);
            report[0] = REPORT_ID;
            report[1..1 + HEADER_LEN].copy_from_slice(&header);
            let dst = &mut report[1 + HEADER_LEN..1 + HEADER_LEN + (end - start)];
            dst.copy_from_slice(&payload[start..end]);

            trace!(chunk = i, total = total_chunks, ?flow, "TFT TX");
            self.device.write(&report)?;

            let acked = self.wait_tft_ack(flow.ack_wait_ms(pace_ms));
            if acked {
                acknowledged += 1;
            } else if flow != TftFlow::Paced {
                debug!(chunk = i, pace_ms, "TFT chunk not acknowledged, pacing");
            }
            flow = flow.next(acked);
        }

        let report = TftUploadReport {
            chunks: total_chunks,
            acknowledged,
            paced_ms: (flow == TftFlow::Paced).then_some(pace_ms as u64),
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        info!(?report, "SET_TFT_USER_ANIMATION done");
        Ok(report)
    }

    /// Wait up to `timeout_ms` for the firmware's per-chunk TFT ack.
    /// Read errors count as "no ack": they must never fail an upload that
    /// would have succeeded fire-and-forget.
    fn wait_tft_ack(&self, timeout_ms: i32) -> bool {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
        let mut buf = [0u8; PACKET_LEN];
        loop {
            let remaining_ms = deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis()
                .min(i32::MAX as u128) as i32;
            if remaining_ms == 0 {
                return false;
            }
            let n = match self.device.read_timeout(&mut buf, remaining_ms) {
                Ok(n) => n,
                Err(e) => {
                    debug!(%e, "TFT ack read failed");
                    return false;
                }
            };
            if n >= 2 && buf[0] == MAGIC_INCOMING && TFT_ACK_CMDS.contains(&buf[1]) {
                return true;
            }
            if n > 0 {
                trace!(
                    magic = buf[0],
                    cmd = buf[1],
                    "ignoring non-ack TFT input report"
                );
            }
        }
    }

    /// Read the 128-LED per-key colour map.
    pub fn get_custom_led(&self) -> Result<CustomLedMap> {
        let payload = self.get_many_at(cmd::GET_CUSTOM_LED_DATA, 0, CUSTOM_LED_BYTES)?;
        Ok(CustomLedMap::decode(&payload))
    }

    /// Write the 128-LED per-key colour map. Caller is responsible for
    /// switching the active lighting effect to one that reads from this
    /// buffer (the standard 20 modes ignore it).
    pub fn set_custom_led(&self, map: &CustomLedMap) -> Result<()> {
        let payload = map.encode();
        info!(bytes = payload.len(), "SET_CUSTOM_LED_DATA");
        self.set_many_at(cmd::SET_CUSTOM_LED_DATA, 0, &payload, true)
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

    fn iface(interface: i32, usage_page: u16) -> DeviceInfo {
        DeviceInfo {
            vid: VENDOR_ID,
            pid: 0x8009,
            interface,
            usage_page,
            usage: 1,
            release_number: 0x0107,
            manufacturer: None,
            product: None,
            serial: None,
            path: format!("iface{interface}-{usage_page:04x}"),
        }
    }

    #[test]
    fn select_control_follows_priority_not_enumeration_order() {
        // 0xFF00 is enumerated first, but 0xFF68 ranks higher.
        let candidates = [iface(0, 0xFF00), iface(3, 0xFF67), iface(2, 0xFF68)];
        let pick = select_control(&candidates).expect("control endpoint");
        assert_eq!(pick.usage_page, 0xFF68);
        assert_eq!(pick.interface, 2);
    }

    #[test]
    fn select_control_falls_back_to_control_interface() {
        let candidates = [iface(1, 0x0001), iface(3, 0xFF67), iface(2, 0x000C)];
        let pick = select_control(&candidates).expect("fallback endpoint");
        assert_eq!(pick.interface, CONTROL_INTERFACE);
    }

    #[test]
    fn select_control_returns_none_without_candidates() {
        assert!(select_control(&[]).is_none());
    }

    #[test]
    fn tft_flow_keeps_waiting_while_acks_arrive() {
        let flow = TftFlow::Probe.next(true);
        assert_eq!(flow, TftFlow::Acked);
        assert_eq!(flow.next(true), TftFlow::Acked);
        assert_eq!(flow.ack_wait_ms(10), TFT_ACK_TIMEOUT_MS);
        assert_eq!(TftFlow::Probe.ack_wait_ms(10), TFT_ACK_PROBE_MS);
    }

    #[test]
    fn tft_flow_paces_without_acks() {
        // Silent firmware: decided after the first chunk.
        assert_eq!(TftFlow::Probe.next(false), TftFlow::Paced);
        // One missing ack later degrades instead of stalling every chunk.
        assert_eq!(TftFlow::Acked.next(false), TftFlow::Paced);
        // Pacing is final for the rest of the upload; the pace window is
        // spent listening for late acks.
        assert_eq!(TftFlow::Paced.next(true), TftFlow::Paced);
        assert_eq!(TftFlow::Paced.ack_wait_ms(25), 25);
    }

    #[test]
    fn feature_report_board_needs_ff13_without_ff67() {
        assert!(looks_like_feature_report_board(&[
            iface(2, 0xFF68),
            iface(3, 0xFF13),
        ]));
        assert!(!looks_like_feature_report_board(&[
            iface(2, 0xFF68),
            iface(3, 0xFF67),
        ]));
        assert!(!looks_like_feature_report_board(&[
            iface(2, 0xFF68),
            iface(3, 0xFF13),
            iface(4, 0xFF67),
        ]));
    }
}
