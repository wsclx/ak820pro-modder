//! Explicit feature availability for the two incompatible wire protocols.
use crate::device::DeviceProtocol;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceCapabilities {
    pub protocol: DeviceProtocol,
    pub host_keyboard: bool,
    pub lighting: bool,
    pub clock_sync: bool,
    pub sleep_timer: bool,
    pub device_info: bool,
    pub game_mode: bool,
    pub keymap: bool,
    pub fn_keymap: bool,
    pub macros: bool,
    pub per_key_rgb: bool,
    pub tft_builtin: bool,
    pub now_playing_tft: bool,
    pub audio_reactive: bool,
}

impl DeviceCapabilities {
    pub fn for_protocol(protocol: DeviceProtocol) -> Self {
        let web = protocol == DeviceProtocol::WebOutput;
        Self {
            protocol,
            host_keyboard: !web && cfg!(target_os = "macos"),
            lighting: true,
            // Family A via cmd 52, family B via the 04 28 transaction.
            clock_sync: true,
            sleep_timer: true,
            device_info: web,
            game_mode: web,
            keymap: web,
            fn_keymap: web,
            macros: web,
            per_key_rgb: true,
            tft_builtin: web,
            now_playing_tft: web,
            audio_reactive: web,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn web_features_stay_enabled_and_legacy_does_not_invent_support() {
        let web = DeviceCapabilities::for_protocol(DeviceProtocol::WebOutput);
        assert!(
            web.lighting
                && web.clock_sync
                && web.device_info
                && web.game_mode
                && web.keymap
                && web.fn_keymap
                && web.macros
                && web.per_key_rgb
                && web.tft_builtin
                && web.now_playing_tft
                && web.audio_reactive
        );
        let legacy = DeviceCapabilities::for_protocol(DeviceProtocol::LegacyFeature);
        assert!(legacy.lighting && legacy.clock_sync && legacy.sleep_timer);
        assert!(legacy.per_key_rgb && !legacy.audio_reactive);
        assert!(!legacy.keymap && !legacy.macros && !legacy.game_mode);
    }
}
