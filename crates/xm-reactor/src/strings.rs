pub const RUNTIME_MISSING: &str = "XM Reactor needs the Windows App Runtime 2.x, which is not installed on this PC.\n\nSelect OK to open the download page, install it, then start XM Reactor again.";
pub const RUNTIME_MISSING_TITLE: &str = "XM Reactor";
pub const WINDOW_TITLE: &str = "XM Reactor";

pub const DEFAULT_DEVICE_NAME: &str = "WH-1000XM6";
/// The simulated headset names itself with this; the marker moves to the status line instead.
pub const DEMO_SUFFIX: &str = " (demo)";
pub const DEMO: &str = "demo";

pub const STATUS_WORN: &str = "Connected · On your head";
pub const STATUS_REMOVED: &str = "Connected · Off your head";
pub const STATUS_CONNECTED: &str = "Connected";
pub const STATUS_CONNECTING: &str = "Connecting…";
pub const STATUS_DISCONNECTED: &str = "Connect the headphones to this PC";
pub const STATUS_NOT_FOUND: &str = "Headphones not found. Are they on?";
pub const STATUS_BLUETOOTH_OFF: &str = "Bluetooth is off";
pub const STATUS_IN_USE: &str = "The headphones are in use by another app";
pub const STATUS_TURNED_OFF: &str = "Turned off. Press the power button to turn them on again.";
pub const OPEN_BLUETOOTH: &str = "Open Bluetooth settings";
pub const TRY_AGAIN: &str = "Try again";
pub const DISCONNECTED_HINT: &str = "Your controls come back when the headphones connect.";

/// The status line with the demo marker after it.
pub fn status_demo(status: &str) -> String {
    format!("{status} · {DEMO}")
}

pub const EQ_REASON_CALL: &str = "The headphones pause the equaliser during calls";
pub const EQ_REASON_BGM: &str = "Background music is on. Switch to Standard to use the equaliser.";
pub const EQ_REASON_CINEMA: &str = "Cinema is on. Switch to Standard to use the equaliser.";
/// The headphones locked the equaliser without saying which listening mode is on.
pub const EQ_REASON_LISTENING_MODE: &str =
    "A listening mode is on. Switch to Standard to use the equaliser.";
/// The headphones refused equaliser changes without a call or listening-mode notice.
pub const EQ_REASON_UNAVAILABLE: &str =
    "The headphones have paused the equaliser, as they do during calls.";
/// Under the curve of a preset the headphones fix.
pub const EQ_BUILT_IN_CAPTION: &str = "Built-in preset";

pub const TILE_NOISE_CANCELLING: &str = "Noise cancelling";
pub const TILE_AMBIENT: &str = "Ambient sound";
pub const TILE_OFF: &str = "Off";
pub const TILE_AUTO_PAUSE: &str = "Pause when taken off";
pub const TILE_SPEAK_TO_CHAT: &str = "Speak-to-chat";
pub const TILE_DSEE: &str = "DSEE";
pub const TILE_SUSPENDED: &str = "Paused while off your head";
pub const NOISE_CONTROL: &str = "Noise control";

// What each tile does, in its tooltip.
pub const TIP_NOISE_CANCELLING: &str = "Blocks out the noise around you";
pub const TIP_AMBIENT: &str =
    "Lets in the sound around you, so you can hear voices and announcements";
pub const TIP_OFF: &str = "Turns off both noise cancelling and ambient sound";
pub const TIP_AUTO_PAUSE: &str =
    "Pauses your music when you take the headphones off and resumes when you put them back on";
pub const TIP_SPEAK_TO_CHAT: &str =
    "Pauses your music and lets in the sound around you when you start talking";
pub const TIP_DSEE: &str = "Restores detail lost in compressed music";

pub const EQ_FLAT: &str = "Flat";
pub const EQ_GAMING: &str = "Gaming";
pub const EQ_HEAVY: &str = "Heavy";
pub const EQ_CLEAR: &str = "Clear";
pub const EQ_HARD: &str = "Hard";
pub const EQ_SOFT: &str = "Soft";
pub const EQ_MANUAL: &str = "Manual";
pub const EQ_CUSTOM1: &str = "Custom 1";
pub const EQ_CUSTOM2: &str = "Custom 2";

pub const APO_WHEN_TAKEN_OFF: &str = "When taken off";
pub const APO_NEVER: &str = "Never";
pub const APO_5_MIN: &str = "After 5 minutes";
pub const APO_30_MIN: &str = "After 30 minutes";
pub const APO_60_MIN: &str = "After 1 hour";
pub const APO_180_MIN: &str = "After 3 hours";

pub const INFO_ADDRESS: &str = "Bluetooth address";
pub const VALUE_ON: &str = "On";
pub const VALUE_OFF: &str = "Off";
pub const VALUE_UNKNOWN: &str = "Unknown";

pub const CODEC_SBC: &str = "SBC";
pub const CODEC_AAC: &str = "AAC";
pub const CODEC_LDAC: &str = "LDAC";
pub const CODEC_APTX: &str = "aptX";
pub const CODEC_APTX_HD: &str = "aptX HD";

pub fn battery(percent: u8) -> String {
    format!("Battery {percent} %")
}

pub fn now_playing(title: &str, artist: &str) -> String {
    format!("{title} · {artist}")
}

pub fn codec_other(code: u8) -> String {
    format!("Codec {code:#04X}")
}

pub const HEADSET_PICTURE: &str = "Headphones";

/// The headset picture's accessible name: the device and the colour shown, such as
/// "WH-1000XM6, Platinum Silver"; just "Headphones" when no picture of them is shown.
pub fn headset_picture(device: &str, colour: Option<&str>) -> String {
    match colour {
        Some(c) if !device.is_empty() => format!("{device}, {c}"),
        _ => HEADSET_PICTURE.to_string(),
    }
}
pub const BATTERY_LEVEL: &str = "Battery level";
pub const CHARGING: &str = "Charging";
pub const AMBIENT_LEVEL: &str = "Ambient sound level";

/// The ambient slider's accessible name carries its value, which reactor cannot expose otherwise.
pub fn ambient_level_name(level: u8, max: u8) -> String {
    format!("{AMBIENT_LEVEL}, {level} of {max}")
}
/// The slider's header with the level in it, as the Windows volume slider shows its number.
pub fn ambient_level_header(level: u8) -> String {
    format!("{AMBIENT_LEVEL} · {level}")
}
pub const FOCUS_ON_VOICE: &str = "Focus on voice";
pub const EQUALISER: &str = "Equaliser";
pub const SETTINGS: &str = "Settings";
pub const MORE_OPTIONS: &str = "More options";
pub const MENU_TURN_OFF: &str = "Turn off headphones…";
pub const MENU_DEMO_ON: &str = "Demo mode · On";
pub const MENU_DEMO_OFF: &str = "Demo mode · Off";
pub const MENU_OPEN_LOG: &str = "Open log";
pub const MENU_QUIT: &str = "Quit XM Reactor";
pub const TURN_OFF_TITLE: &str = "Turn off your headphones?";
pub const TURN_OFF_CONFIRM: &str = "Turn off";
pub const TURN_OFF_CANCEL: &str = "Cancel";
pub const TURN_OFF_BODY: &str = "Press the power button to turn them on again.";

/// A tile's accessible name: its label, then its subtitle when it has one.
pub fn tile_name(label: &str, subtitle: Option<&str>) -> String {
    match subtitle {
        Some(sub) => format!("{label}, {sub}"),
        None => label.to_string(),
    }
}

pub fn battery_charging(battery: &str) -> String {
    format!("{battery} · {CHARGING}")
}

pub const BACK: &str = "Back";
pub const NOT_CONNECTED_NOTE: &str = "Connect the headphones to change these settings.";
pub const SECTION_HEADPHONES: &str = "Headphones";
pub const SECTION_ON_HEADPHONES: &str = "On the headphones";
pub const SECTION_APP: &str = "App";
pub const SECTION_ABOUT: &str = "About";
pub const NO_HEADPHONE_SETTINGS: &str = "These headphones offer nothing to change here.";

pub const RENAME: &str = "Rename";
pub const RENAME_PRESET: &str = "Rename preset";
pub const RENAME_CANCEL: &str = "Cancel";
pub const PRESET_NAME: &str = "Preset name";
pub const RENAME_HINT: &str =
    "Up to 24 characters, shown only in XM Reactor. Leave it empty to use the preset's original name.";
/// A custom preset's accessible name when its nickname matches another preset's name.
pub fn custom_preset_name(label: &str) -> String {
    format!("{label} (custom)")
}
pub const EQ_CURVE: &str = "Equaliser curve";
pub const EQ_LOCKED: &str = ", locked";
pub const EQ_BUILT_IN: &str = ", built-in preset";
pub const CLEAR_BASS: &str = "Clear Bass";
pub const RESET_FLAT: &str = "Reset to flat";
pub const LISTENING_MODE: &str = "Listening mode";
pub const LM_STANDARD: &str = "Standard";
pub const LM_BGM: &str = "Background music";
pub const LM_CINEMA: &str = "Cinema";
pub const ROOM: &str = "Room";
pub const ROOM_MY: &str = "My room";
pub const ROOM_LIVING: &str = "Living room";
pub const ROOM_CAFE: &str = "Café";

pub const AUTO_POWER_OFF: &str = "Auto power off";
pub const CAPTURE_VOICE: &str = "Capture voice during calls";
pub const TOUCH_CONTROLS: &str = "Touch controls";
pub const NC_BUTTON: &str = "Noise control button";
pub const NC_BUTTON_NC_AMB: &str = "Noise cancelling ↔ Ambient";
pub const VOICE_GUIDANCE: &str = "Voice guidance volume";
pub const MULTIPOINT: &str = "Multipoint";
pub const READ_ONLY_NOTE: &str = "Change these on the headphones";
pub const CONNECTED_DEVICES: &str = "Connected devices";
pub const SOURCE_PLAYING: &str = "Playing";
pub const SOURCE_CONNECTED: &str = "Connected";
pub const SOURCE_NOT_CONNECTED: &str = "Not connected";
pub const PLAY_HERE: &str = "Play here";

pub const HEADSET_COLOUR: &str = "Headset colour";
pub const COLOUR_AUTO: &str = "Automatic";
pub const COLOUR_SILVER: &str = "Platinum Silver";
pub const COLOUR_BLUE: &str = "Midnight Blue";
pub const COLOUR_BLACK: &str = "Black";
pub const BATTERY_IN_TOOLTIP: &str = "Battery percentage in tooltip";
pub const LOW_BATTERY_NOTIFICATION: &str = "Low-battery notification";
pub const OPEN_AT_SIGN_IN: &str = "Open at sign-in";
pub const THEME: &str = "Theme";
pub const THEME_FOLLOW: &str = "Follow Windows";
pub const THEME_LIGHT: &str = "Light";
pub const THEME_DARK: &str = "Dark";

pub const ABOUT_VERSION: &str = "XM Reactor version";
pub const ABOUT_NAME: &str = "Name";
pub const ABOUT_FIRMWARE: &str = "Firmware";
pub const ABOUT_CODEC: &str = "Bluetooth codec";
pub const OPEN_LOG_FOLDER: &str = "Open log folder";
pub const LICENCES: &str = "Licences";
/// The crates linked into the app, from Cargo.lock (build-time macros excluded).
pub const LICENCE_APP: &str = "XM Reactor is open source under the MIT licence.";
pub const LICENCES_TEXT: &str = "XM Reactor uses these Rust crates under the MIT licence or the \
Apache License 2.0: itoa, memchr, serde, serde_core, serde_json, thiserror, zmij, windows and windows-reactor, \
with their companion crates windows-collections, windows-core, windows-future, windows-link, \
windows-numerics, windows-reference, windows-result, windows-strings, windows-threading, \
windows-time and windows-window.";

/// Signed, so a level below the default reads "-1" and above it "+1".
pub fn voice_guidance(level: i8) -> String {
    if level == 0 {
        "0".into()
    } else {
        format!("{level:+}")
    }
}

/// A Play here button's accessible name, unique per device.
pub fn play_here_name(device: &str) -> String {
    format!("{PLAY_HERE} · {device}")
}

/// "31" reads "31 Hz band" and "1k" reads "1 kHz band" to a screen reader.
pub fn band_name(label: &str) -> String {
    match label.strip_suffix('k') {
        Some(khz) => format!("{khz} kHz band"),
        None => format!("{label} Hz band"),
    }
}

/// A band's name with its gain, such as "63 Hz band, +3 dB".
pub fn band_value_name(label: &str, gain: i8) -> String {
    let gain = if gain == 0 {
        "0".to_string()
    } else {
        format!("{gain:+}")
    };
    format!("{}, {gain} dB", band_name(label))
}

/// The equaliser curve's accessible name with its selected band, such as "Equaliser curve, 1 kHz
/// band, +3 dB", then `note`: `EQ_LOCKED`, `EQ_BUILT_IN` or nothing.
pub fn curve_name(label: &str, gain: i8, note: &str) -> String {
    format!("{EQ_CURVE}, {}{note}", band_value_name(label, gain))
}

// One-line descriptions under each settings row.
pub const AUTO_POWER_OFF_DESC: &str = "When the headphones turn themselves off to save battery";
pub const CAPTURE_VOICE_DESC: &str = "Hear your own voice in the headphones during calls";
pub const TOUCH_CONTROLS_DESC: &str = "Tap and swipe the ear cup to control playback";
pub const NC_BUTTON_DESC: &str = "What the NC/AMB button on the headphones switches between";
pub const VOICE_GUIDANCE_DESC: &str = "How loud the headphones' spoken prompts are";
pub const MULTIPOINT_DESC: &str = "Stay connected to two devices at once";
pub const ADDRESS_DESC: &str = "The headphones' Bluetooth hardware address";
pub const CONNECTED_DEVICES_DESC: &str =
    "Devices the headphones are connected to. Play here moves playback to one.";
pub const HEADSET_COLOUR_DESC: &str =
    "The picture on the main page. Automatic uses the colour the headphones report.";
pub const BATTERY_IN_TOOLTIP_DESC: &str = "Show the battery level when you point at the tray icon";
pub const LOW_BATTERY_DESC: &str = "Let you know once when the battery drops below 20 %";
pub const OPEN_AT_SIGN_IN_DESC: &str = "Start XM Reactor in the tray when you sign in to Windows";
pub const THEME_DESC: &str = "Light, dark, or the same as Windows";

pub const HOW_IT_WORKS: &str = "How it works";
pub const HOW_IT_WORKS_TEXT: &str = "XM Reactor talks to your headphones directly over Bluetooth \
from this PC. There's no account to create, and nothing is sent over the internet.";

// Tray icon and its menu. Reactor menus have no check mark, so the state is in the label.
pub const TRAY_AMBIENT: &str = "Ambient";

/// The tray menu's mode item for the mode the headphones are in.
pub fn tray_current(mode: &str) -> String {
    format!("{mode} · current")
}

pub fn tray_open_at_sign_in(on: bool) -> String {
    format!(
        "{OPEN_AT_SIGN_IN} · {}",
        if on { VALUE_ON } else { VALUE_OFF }
    )
}

/// The tray tooltip while connected: the battery appears only when known and wanted.
pub fn tray_tooltip(device: &str, battery: Option<u8>) -> String {
    match battery {
        Some(p) => format!("{WINDOW_TITLE} · {device} · {p} %"),
        None => format!("{WINDOW_TITLE} · {device}"),
    }
}

pub fn tray_tooltip_status(status: &str) -> String {
    format!("{WINDOW_TITLE} · {status}")
}

// Low-battery notification.
pub fn low_battery_title(device: &str) -> String {
    format!("{device} battery is low")
}

pub fn low_battery_body(level: u8) -> String {
    format!("{level} % left. Charge them soon.")
}

/// Asked by `--uninstall-cleanup` unless `--quiet`.
pub const CLEANUP_DELETE_DATA: &str = "Also delete XM Reactor's settings and logs?";
