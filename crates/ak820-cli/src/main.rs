use ak820_protocol::commands::lighting::{
    led_effect_payload, parse_hex_rgb, Direction, LightingConfig, Mode, MAX_BRIGHTNESS, MAX_SPEED,
};
use ak820_protocol::commands::per_key_rgb::{CustomLedMap, LedColor, LED_COUNT};
use ak820_protocol::commands::tft::{TftAnimation, TftFrame, PIXELS_PER_FRAME};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "ak820",
    version,
    about = "Control CLI for the Epomaker / Ajazz AK820 Pro"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// Emit JSON instead of human-readable output
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// List every HID interface the keyboard exposes
    List,
    /// Open the control interface and run a probe handshake
    Probe,
    /// Lighting control
    #[command(subcommand)]
    Lighting(LightingCmd),
    /// Read device info (firmware, battery, profile, …)
    Info,
    /// Read or write the game-mode struct (sleep timer, key delay, …)
    #[command(subcommand)]
    GameMode(GameModeCmd),
    /// Read macros from the device (read-only; write path lives in the UI)
    #[command(subcommand)]
    Macros(MacrosCmd),
    /// Per-key RGB control (cmd 36 SET_CUSTOM_LED_DATA + mode 0x80)
    #[command(subcommand)]
    Rgb(RgbCmd),
    /// Inspect every HID interface's report descriptor — used to identify
    /// the right endpoint for big writes (TFT animation upload, etc.).
    HidDescriptors,
    /// TFT display upload (128×128 RGB565 frames; cmd 80 on the 0xFF67 endpoint)
    #[command(subcommand)]
    Tft(TftCmd),
}

#[derive(Subcommand)]
enum TftCmd {
    /// Upload a single solid-colour frame to the TFT.
    Solid {
        #[arg(long, default_value = "FF00FF")]
        color: String,
    },
    /// Upload an animated 6-frame RGB cycle (red → orange → yellow → green → blue → magenta).
    /// Each frame lingers for `delay` ms.
    Cycle {
        #[arg(long, default_value_t = 200)]
        delay: u16,
    },
    /// Select which TFT animation plays. The default factory built-in is
    /// usually slot 0; the user-uploaded slot lives just past the
    /// built-in count. Try values 0..10 until one renders your upload.
    SelectIndex {
        #[arg(long)]
        index: u8,
    },
}

#[derive(Subcommand)]
enum MacrosCmd {
    /// List every macro stored on the device
    List,
}

#[derive(Subcommand)]
enum RgbCmd {
    /// Paint every key a single colour (6-char hex, default FF00FF). Also
    /// switches the keyboard's lighting mode to `custom` so the buffer is
    /// rendered.
    Fill {
        /// 6-char hex (e.g. `FF00AA`).
        #[arg(long, default_value = "FF00FF")]
        color: String,
    },
    /// Paint a rainbow hue gradient across all 128 LEDs — quick visual
    /// confirmation that the per-key buffer + custom mode work end-to-end.
    Rainbow,
}

#[derive(Subcommand)]
enum GameModeCmd {
    /// Read and print the current game-mode struct
    Get,
    /// Set the sleep-timer preset (read-modify-write of the full struct)
    SetSleep {
        /// One of 0,1,2,3,4,5 (= never, 1m, 5m, 10m, 15m, 30m).
        #[arg(long)]
        value: u8,
    },
}

#[derive(Subcommand)]
enum LightingCmd {
    /// Print the 20 supported lighting modes
    Modes,
    /// Apply a lighting configuration
    Set {
        /// Mode name (see `lighting modes`)
        #[arg(long)]
        mode: String,
        /// 6-char hex color (default FFFFFF).
        #[arg(long, default_value = "FFFFFF")]
        color: String,
        /// Optional secondary RGB for dual-colour effects.
        #[arg(long)]
        secondary: Option<String>,
        /// `colorMode` byte (0 = monochrome). Increase for cycling palettes.
        #[arg(long, default_value_t = 0)]
        color_mode: u8,
        /// `effectModeType` byte at payload offset 12.
        #[arg(long, default_value_t = 0)]
        effect_mode_type: u8,
        /// Brightness 0–5
        #[arg(long, default_value_t = 3)]
        brightness: u8,
        /// Speed 0–5
        #[arg(long, default_value_t = 3)]
        speed: u8,
        /// Direction (left/right/up/down). Only honoured by directional modes.
        #[arg(long, default_value = "left")]
        direction: String,
        /// Log the would-be bytes without sending
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => cmd_list(cli.json),
        Cmd::Probe => cmd_probe(cli.json),
        Cmd::Info => cmd_info(cli.json),
        Cmd::GameMode(GameModeCmd::Get) => cmd_game_mode_get(cli.json),
        Cmd::GameMode(GameModeCmd::SetSleep { value }) => cmd_game_mode_set_sleep(cli.json, value),
        Cmd::Macros(MacrosCmd::List) => cmd_macros_list(cli.json),
        Cmd::Rgb(RgbCmd::Fill { color }) => cmd_rgb_fill(cli.json, &color),
        Cmd::Rgb(RgbCmd::Rainbow) => cmd_rgb_rainbow(cli.json),
        Cmd::HidDescriptors => cmd_hid_descriptors(cli.json),
        Cmd::Tft(TftCmd::Solid { color }) => cmd_tft_solid(cli.json, &color),
        Cmd::Tft(TftCmd::Cycle { delay }) => cmd_tft_cycle(cli.json, delay),
        Cmd::Tft(TftCmd::SelectIndex { index }) => cmd_tft_select_index(cli.json, index),
        Cmd::Lighting(LightingCmd::Modes) => cmd_lighting_modes(cli.json),
        Cmd::Lighting(LightingCmd::Set {
            mode,
            color,
            secondary,
            color_mode,
            effect_mode_type,
            brightness,
            speed,
            direction,
            dry_run,
        }) => cmd_lighting_set(
            cli.json,
            &mode,
            &color,
            secondary.as_deref(),
            color_mode,
            effect_mode_type,
            brightness,
            speed,
            &direction,
            dry_run,
        ),
    }
}

fn cmd_list(json: bool) -> Result<()> {
    let devices = ak820_protocol::enumerate()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&devices)?);
        return Ok(());
    }
    if devices.is_empty() {
        println!(
            "No AK820 candidates found (VID=0x{:04x}, PIDs={}).",
            ak820_protocol::VENDOR_ID,
            ak820_protocol::PRODUCT_IDS
                .iter()
                .map(|p| format!("0x{:04x}", p))
                .collect::<Vec<_>>()
                .join(", "),
        );
        return Ok(());
    }
    println!("Found {} interface(s):", devices.len());
    for d in devices {
        println!(
            "  iface={} usage_page=0x{:04x} usage=0x{:04x}  pid=0x{:04x}  {}  {}",
            d.interface,
            d.usage_page,
            d.usage,
            d.pid,
            d.product.as_deref().unwrap_or("?"),
            d.path,
        );
    }
    Ok(())
}

fn cmd_probe(json: bool) -> Result<()> {
    let conn = ak820_protocol::Connection::open_control()?;
    let report = conn.probe()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Connected: {}", report.connected);
        println!("Interface: {}", report.interface);
        println!("Product:   {}", report.product.as_deref().unwrap_or("?"));
        println!(
            "Firmware:  {}",
            report
                .firmware_version
                .as_deref()
                .unwrap_or("<not decoded yet>")
        );
    }
    Ok(())
}

fn cmd_lighting_modes(json: bool) -> Result<()> {
    if json {
        let names: Vec<&'static str> = Mode::ALL.iter().map(|m| m.name()).collect();
        println!("{}", serde_json::to_string_pretty(&names)?);
        return Ok(());
    }
    println!("Lighting modes:");
    for m in Mode::ALL {
        let dirs = m.supported_directions();
        if dirs.is_empty() {
            println!("  {:<13}  (direction ignored)", m.name());
        } else {
            let d: Vec<&str> = dirs.iter().map(direction_name).collect();
            println!("  {:<13}  directions: {}", m.name(), d.join(", "));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_lighting_set(
    json: bool,
    mode_name: &str,
    color: &str,
    secondary: Option<&str>,
    color_mode: u8,
    effect_mode_type: u8,
    brightness: u8,
    speed: u8,
    direction: &str,
    dry_run: bool,
) -> Result<()> {
    let mode = Mode::from_name(mode_name)
        .with_context(|| format!("unknown lighting mode `{}`", mode_name))?;
    let direction = Direction::from_name(direction)
        .with_context(|| format!("unknown direction `{}`", direction))?;
    if parse_hex_rgb(color).is_none() {
        bail!("--color must be 6 hex digits, got `{}`", color);
    }
    if let Some(s) = secondary {
        if parse_hex_rgb(s).is_none() {
            bail!("--secondary must be 6 hex digits, got `{}`", s);
        }
    }
    if brightness > MAX_BRIGHTNESS {
        bail!("--brightness must be 0–{}", MAX_BRIGHTNESS);
    }
    if speed > MAX_SPEED {
        bail!("--speed must be 0–{}", MAX_SPEED);
    }

    let cfg = LightingConfig {
        mode,
        color: color.to_owned(),
        secondary: secondary.map(str::to_owned),
        color_mode,
        effect_mode_type,
        brightness,
        speed,
        direction,
    };

    if dry_run {
        let payload = led_effect_payload(&cfg);
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "dry_run": true,
                    "mode": mode.name(),
                    "payload_hex": hex::encode(payload),
                })
            );
        } else {
            println!("DRY RUN — would send SET_LED_EFFECT (cmd 0x23):");
            println!(
                "  mode={} brightness={} speed={} direction={:?}",
                mode.name(),
                brightness,
                speed,
                cfg.direction
            );
            println!("  payload (16 B): {}", hex::encode(payload));
        }
        return Ok(());
    }

    let conn = ak820_protocol::Connection::open_control()?;
    conn.set_lighting(&cfg)?;
    if json {
        println!("{}", serde_json::json!({"ok": true, "mode": mode.name()}));
    } else {
        println!(
            "Lighting set: mode={} brightness={} speed={}",
            mode.name(),
            brightness,
            speed,
        );
    }
    Ok(())
}

fn direction_name(d: &Direction) -> &'static str {
    match d {
        Direction::Left => "left",
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::Right => "right",
    }
}

fn cmd_info(json: bool) -> Result<()> {
    let conn = ak820_protocol::Connection::open_control()?;
    let info = conn.get_device_info()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("Firmware:        v{:.2}", info.firmware_version);
        println!("VID/PID:         0x{:04x}:0x{:04x}", info.vid, info.pid);
        println!("Battery level:   {}%", info.battery_level);
        println!("Charge status:   {}", info.charge_status);
        println!("Current profile: {}", info.current_profile);
        println!("Macro space:     {} bytes", info.macro_space_size);
        println!("Frame version:   {}", info.frame_version);
        println!("TFT max frames:  {}", info.tft_max_frames);
    }
    Ok(())
}

fn cmd_game_mode_get(json: bool) -> Result<()> {
    let conn = ak820_protocol::Connection::open_control()?;
    let gm = conn.get_game_mode()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&gm)?);
    } else {
        println!("Game mode:        {}", gm.game_mode);
        println!(
            "Sleep timer:      {} (0=never,1=1m,2=5m,3=10m,4=15m,5=30m)",
            gm.sleep_time
        );
        println!("Key delay:        {}", gm.key_delay);
        println!("Report rate:      {}", gm.report_rate);
        println!("TFT display time: {}", gm.tft_display_time);
        println!("Stability mode:   {}", gm.stability_mode);
        println!("Auto calibration: {}", gm.auto_calibration);
    }
    Ok(())
}

fn cmd_tft_solid(json: bool, color: &str) -> Result<()> {
    let (r, g, b) = parse_hex_rgb(color).ok_or_else(|| anyhow::anyhow!("invalid --color"))?;
    let mut rgb = Vec::with_capacity(PIXELS_PER_FRAME * 3);
    for _ in 0..PIXELS_PER_FRAME {
        rgb.extend_from_slice(&[r, g, b]);
    }
    let frame = TftFrame::from_rgb888(&rgb, 0)?;
    let anim = TftAnimation {
        frames: vec![frame],
    };
    let tft = ak820_protocol::Connection::open_tft()?;
    tft.upload_tft_animation(&anim)?;
    if json {
        println!(
            "{}",
            serde_json::json!({"ok": true, "frames": 1, "color": format!("{:02X}{:02X}{:02X}", r, g, b)})
        );
    } else {
        println!(
            "TFT solid #{:02X}{:02X}{:02X} uploaded (1 frame, ~33 KB).",
            r, g, b
        );
        println!("If the display still shows the default animation, run:");
        println!("    ak820 tft select-index --index N   (try 0, then 1, …)");
    }
    Ok(())
}

fn cmd_tft_select_index(json: bool, index: u8) -> Result<()> {
    let ctrl = ak820_protocol::Connection::open_control()?;
    ctrl.set_tft_built_in_index(index)?;
    if json {
        println!("{}", serde_json::json!({"ok": true, "index": index}));
    } else {
        println!("TFT playback index set to {}", index);
    }
    Ok(())
}

fn cmd_tft_cycle(json: bool, delay_ms: u16) -> Result<()> {
    let palette: [(u8, u8, u8); 6] = [
        (0xFF, 0x00, 0x00),
        (0xFF, 0x80, 0x00),
        (0xFF, 0xFF, 0x00),
        (0x00, 0xC0, 0x40),
        (0x00, 0x60, 0xFF),
        (0xFF, 0x00, 0xC0),
    ];
    let mut frames = Vec::with_capacity(palette.len());
    for (r, g, b) in palette {
        let mut rgb = Vec::with_capacity(PIXELS_PER_FRAME * 3);
        for _ in 0..PIXELS_PER_FRAME {
            rgb.extend_from_slice(&[r, g, b]);
        }
        frames.push(TftFrame::from_rgb888(&rgb, delay_ms)?);
    }
    let anim = TftAnimation { frames };
    let conn = ak820_protocol::Connection::open_tft()?;
    conn.upload_tft_animation(&anim)?;
    if json {
        println!(
            "{}",
            serde_json::json!({"ok": true, "frames": 6, "delay_ms": delay_ms})
        );
    } else {
        println!("TFT 6-frame colour cycle uploaded (delay {} ms).", delay_ms);
    }
    Ok(())
}

fn cmd_hid_descriptors(json: bool) -> Result<()> {
    let probes = ak820_protocol::probe_interfaces()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&probes)?);
        return Ok(());
    }
    println!("Inspected {} HID interface(s):", probes.len());
    for p in &probes {
        let info = &p.info;
        let max = p
            .max_output_report_bytes
            .map(|n| format!("{} B", n))
            .unwrap_or_else(|| "?".to_owned());
        println!(
            "  iface={} usage_page=0x{:04x} usage=0x{:04x}  max output report: {:>6}   {}",
            info.interface,
            info.usage_page,
            info.usage,
            max,
            info.product.as_deref().unwrap_or("?"),
        );
    }
    let big = probes
        .iter()
        .filter(|p| p.max_output_report_bytes.unwrap_or(0) >= 1024)
        .collect::<Vec<_>>();
    if big.is_empty() {
        println!();
        println!(
            "No interface advertises ≥1 KB output reports. TFT upload will need a different strategy."
        );
    } else {
        println!();
        println!(
            "Likely TFT upload target(s) (≥1 KB output): iface(s) {}",
            big.iter()
                .map(|p| p.info.interface.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    Ok(())
}

fn cmd_rgb_fill(json: bool, color: &str) -> Result<()> {
    let (r, g, b) = parse_hex_rgb(color).ok_or_else(|| anyhow::anyhow!("invalid --color"))?;
    let mut map = CustomLedMap::default();
    for i in 0..LED_COUNT {
        map.set(i, r, g, b);
    }
    apply_custom(json, &map, &format!("fill #{:02X}{:02X}{:02X}", r, g, b))
}

fn cmd_rgb_rainbow(json: bool) -> Result<()> {
    let mut map = CustomLedMap::default();
    for i in 0..LED_COUNT {
        // Cheap HSV→RGB rainbow across the 128 LEDs.
        let hue = (i as f32) / (LED_COUNT as f32);
        let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
        map.set(i, r, g, b);
    }
    apply_custom(json, &map, "rainbow")
}

fn apply_custom(json: bool, map: &CustomLedMap, label: &str) -> Result<()> {
    let conn = ak820_protocol::Connection::open_control()?;

    // Switch the active lighting mode to 0x80 (Custom) so the firmware
    // reads from the per-key buffer. Brightness/speed/colour fields are
    // ignored in this mode but we set sensible values anyway.
    let cfg = LightingConfig {
        mode: Mode::Custom,
        color: "FFFFFF".into(),
        secondary: None,
        color_mode: 0,
        effect_mode_type: 0,
        brightness: MAX_BRIGHTNESS,
        speed: 0,
        direction: Direction::Left,
    };
    conn.set_lighting(&cfg)?;
    conn.set_custom_led(map)?;

    if json {
        let sample: Vec<&LedColor> = map.leds.iter().take(3).collect();
        println!(
            "{}",
            serde_json::json!({"ok": true, "label": label, "sample_first_3": sample})
        );
    } else {
        println!(
            "Per-key RGB applied: {} (mode=custom 0x80, 128 LEDs × 4 B = 512 B)",
            label,
        );
    }
    Ok(())
}

/// Tiny HSV→RGB for the rainbow CLI helper. `h, s, v` in [0,1].
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let i = (h * 6.0).floor() as i32;
    let f = h * 6.0 - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn cmd_macros_list(json: bool) -> Result<()> {
    let conn = ak820_protocol::Connection::open_control()?;
    let macros = conn.get_macros()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&macros)?);
        return Ok(());
    }
    if macros.is_empty() {
        println!("No macros stored on the device.");
        return Ok(());
    }
    println!("{} macro slot(s) in use:", macros.len());
    for m in &macros {
        println!(
            "  #{:>3}  {:>3} action(s){}",
            m.macro_id,
            m.actions.len(),
            if m.actions.is_empty() {
                "  (empty placeholder)"
            } else {
                ""
            },
        );
        for (i, a) in m.actions.iter().enumerate().take(8) {
            println!(
                "        [{:>2}] {} key=0x{:02x} delay={}ms kind={:?}",
                i,
                if a.is_press { "DOWN" } else { "UP  " },
                a.key_code,
                a.delay_ms,
                a.kind,
            );
        }
        if m.actions.len() > 8 {
            println!("        … {} more", m.actions.len() - 8);
        }
    }
    Ok(())
}

fn cmd_game_mode_set_sleep(json: bool, value: u8) -> Result<()> {
    if value > 5 {
        bail!("--value must be 0–5");
    }
    let conn = ak820_protocol::Connection::open_control()?;
    let mut gm = conn.get_game_mode()?;
    let prev = gm.sleep_time;
    gm.sleep_time = value;
    conn.set_game_mode(&gm)?;
    if json {
        println!(
            "{}",
            serde_json::json!({"ok": true, "previous": prev, "new": value})
        );
    } else {
        println!("Sleep timer changed: {} → {}", prev, value);
    }
    Ok(())
}
