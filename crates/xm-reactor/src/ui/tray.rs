//! The notification-area icon: its glyph, tooltip and menu, from the latest `PanelVm`.

use crate::strings;
use crate::view_model::{PanelVm, TileKind};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows_reactor::*;
use xm_core::log_info;
use xm_core::settings;

/// What the panel hands the application to declare the notify icon.
#[derive(Debug, Clone, PartialEq)]
pub struct Tray {
    pub icon: PathBuf,
    pub tooltip: String,
    pub menu: Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Mode(TileKind),
    ToggleOpenAtSignIn,
    Quit,
}

const SIGN_IN: &str = "open-at-sign-in";
const QUIT: &str = "quit";
/// The shell refuses a longer tooltip.
const TOOLTIP_MAX_UTF16: usize = 127;

/// Menu key, tile and label of each mode item, in menu order.
const MODES: [(&str, TileKind, &str); 3] = [
    (
        "nc",
        TileKind::NoiseCancelling,
        strings::TILE_NOISE_CANCELLING,
    ),
    ("ambient", TileKind::Ambient, strings::TRAY_AMBIENT),
    ("off", TileKind::Off, strings::TILE_OFF),
];

macro_rules! icons {
    ($($name:literal),*) => {
        [$(($name, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/tray/", $name)) as &[u8])),*]
    };
}

/// Glyph by glyph, the dark-taskbar file then the light-taskbar one (made by xm-tools' tray-icons).
const ICONS: [(&str, &[u8]); 8] = icons!(
    "nc-dark.ico",
    "nc-light.ico",
    "ambient-dark.ico",
    "ambient-light.ico",
    "off-dark.ico",
    "off-light.ico",
    "disconnected-dark.ico",
    "disconnected-light.ico"
);

/// The icon file for the mode and taskbar theme; any connected state without a known mode is Off.
pub fn tray_icon_choice(
    mode: Option<TileKind>,
    connected: bool,
    light_taskbar: bool,
) -> &'static str {
    let glyph = match (connected, mode) {
        (false, _) => 3,
        (true, Some(TileKind::NoiseCancelling)) => 0,
        (true, Some(TileKind::Ambient)) => 1,
        _ => 2,
    };
    ICONS[glyph * 2 + usize::from(light_taskbar)].0
}

/// The noise-control mode the headphones are in, from the mode tiles.
fn current_mode(vm: &PanelVm) -> Option<TileKind> {
    let is_mode = |k| MODES.iter().any(|(_, m, _)| *m == k);
    vm.tiles
        .iter()
        .find(|t| t.on && is_mode(t.kind))
        .map(|t| t.kind)
}

pub fn tooltip_text(vm: Option<&PanelVm>) -> String {
    let text = match vm {
        None => strings::WINDOW_TITLE.to_string(),
        Some(vm) if vm.connected => {
            let battery = vm
                .battery_fraction
                .filter(|_| vm.settings.app.battery_in_tooltip)
                .map(|f| (f * 100.0).round() as u8);
            strings::tray_tooltip(&vm.device_name, battery)
        }
        Some(vm) => strings::tray_tooltip_status(&vm.status),
    };
    let mut units = 0;
    // The shell would reject a NUL too, which stops the app.
    text.chars()
        .filter(|&c| c != '\0')
        .take_while(|c| {
            units += c.len_utf16();
            units <= TOOLTIP_MAX_UTF16
        })
        .collect()
}

/// Mode items (connected only; the current one marked), then Open at sign-in and Quit.
pub fn menu_items(vm: Option<&PanelVm>, open_at_sign_in: bool) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if let Some(vm) = vm.filter(|vm| vm.connected) {
        for (key, kind, label) in MODES {
            let Some(t) = vm.tiles.iter().find(|t| t.kind == kind && t.visible) else {
                continue;
            };
            let label = if t.on {
                strings::tray_current(label)
            } else {
                label.to_string()
            };
            items.push(if t.enabled {
                MenuItem::item(key, label)
            } else {
                MenuItem::disabled(key, label)
            });
        }
        if !items.is_empty() {
            items.push(MenuItem::separator("sep-1"));
        }
    }
    items.extend([
        MenuItem::item(SIGN_IN, strings::tray_open_at_sign_in(open_at_sign_in)),
        MenuItem::separator("sep-2"),
        MenuItem::item(QUIT, strings::MENU_QUIT),
    ]);
    items
}

/// The action for a tray menu key (keys are not `Send`, so the panel's message carries this).
pub fn menu_action(k: &Key) -> Option<TrayAction> {
    match k.as_str()? {
        SIGN_IN => Some(TrayAction::ToggleOpenAtSignIn),
        QUIT => Some(TrayAction::Quit),
        k => MODES
            .iter()
            .find(|(key, _, _)| *key == k)
            .map(|(_, m, _)| TrayAction::Mode(*m)),
    }
}

/// The fallback icon folder's name under `%TEMP%`.
pub const TEMP_ICON_DIR: &str = "XM Reactor tray";

/// The folder the icons were written to, since the shell loads tray icons from files; None (and
/// no tray icon) when they could not be written.
pub fn icon_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dirs = [
            settings::app_dir().join("tray"),
            std::env::temp_dir().join(TEMP_ICON_DIR),
        ];
        dirs.into_iter().find(|dir| match write_icons(dir) {
            Ok(()) => true,
            Err(e) => {
                log_info!("could not write the tray icons to {}: {e}", dir.display());
                false
            }
        })
    })
    .as_deref()
}

/// Writes only the icons whose files differ, so correct files (even read-only ones) are kept.
fn write_icons(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, bytes) in ICONS {
        let path = dir.join(name);
        if std::fs::read(&path).ok().as_deref() != Some(bytes) {
            std::fs::write(&path, bytes)?;
        }
    }
    Ok(())
}

/// The tray for the latest view model; None when the icon files are missing.
pub fn tray(
    vm: Option<&PanelVm>,
    open_at_sign_in: bool,
    light_taskbar: bool,
    on_menu: &Callback<Key>,
) -> Option<Tray> {
    let connected = vm.is_some_and(|vm| vm.connected);
    let icon = tray_icon_choice(vm.and_then(current_mode), connected, light_taskbar);
    Some(Tray {
        icon: icon_dir()?.join(icon),
        tooltip: tooltip_text(vm),
        menu: Menu::new(menu_items(vm, open_at_sign_in), on_menu.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_model::tests::{base, ready};
    use crate::view_model::{build, TileVm};
    use xm_core::session::SessionState;
    use xm_core::settings::Settings;

    fn connected(battery_in_tooltip: bool) -> PanelVm {
        let s = Settings {
            battery_in_tooltip,
            ..Settings::default()
        };
        build(&ready(), &s, false, false)
    }

    fn disconnected() -> PanelVm {
        build(
            &base(SessionState::Disconnected),
            &Settings::default(),
            false,
            false,
        )
    }

    /// (key, label) of each item; separators read ("-", "").
    fn entries(items: &[MenuItem]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|i| match i {
                MenuItem::Item { key, label, .. } => {
                    (key.as_str().unwrap_or("").to_string(), label.to_string())
                }
                _ => ("-".into(), String::new()),
            })
            .collect()
    }

    #[test]
    fn tooltip_text_connected_with_battery() {
        assert_eq!(
            tooltip_text(Some(&connected(true))),
            "XM Reactor · WH-1000XM6 · 57 %"
        );
    }

    #[test]
    fn tooltip_text_connected_without_battery() {
        assert_eq!(
            tooltip_text(Some(&connected(false))),
            "XM Reactor · WH-1000XM6"
        );
    }

    #[test]
    fn tooltip_text_disconnected_shows_status() {
        assert_eq!(
            tooltip_text(Some(&disconnected())),
            "XM Reactor · Connect the headphones to this PC"
        );
        assert_eq!(tooltip_text(None), "XM Reactor");
    }

    #[test]
    fn tooltip_text_fits_the_shell_limit() {
        let mut vm = connected(true);
        vm.device_name = "Ä".repeat(200);
        let t = tooltip_text(Some(&vm));
        assert!(t.encode_utf16().count() <= 127);
        assert!(t.starts_with("XM Reactor · ÄÄ"));
    }

    #[test]
    fn tooltip_text_never_splits_a_character_or_keeps_a_nul() {
        let mut vm = connected(false);
        vm.device_name = format!("a\0{}", "🎧".repeat(100));
        let t = tooltip_text(Some(&vm));
        assert!(!t.contains('\0'));
        // "XM Reactor · a" is 14 units; each emoji is a surrogate pair, so 56 fit in 127 and the
        // 57th, which would straddle the limit, is dropped whole.
        assert_eq!(t, format!("XM Reactor · a{}", "🎧".repeat(56)));
        assert_eq!(t.encode_utf16().count(), 126);
    }

    #[test]
    fn icons_are_rewritten_only_when_they_differ() {
        let dir = std::env::temp_dir().join(format!("xm-tray-test-{}", std::process::id()));
        write_icons(&dir).unwrap();
        let (name, bytes) = ICONS[0];
        let path = dir.join(name);
        let result = std::panic::catch_unwind(|| {
            // A correct read-only file is left alone rather than failing the write.
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_readonly(true);
            std::fs::set_permissions(&path, perms).unwrap();
            write_icons(&dir).unwrap();
            // A damaged file is replaced.
            let other = dir.join(ICONS[1].0);
            std::fs::write(&other, b"junk").unwrap();
            write_icons(&dir).unwrap();
            assert_eq!(std::fs::read(&other).unwrap(), ICONS[1].1);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        });
        if let Ok(m) = std::fs::metadata(&path) {
            let mut perms = m.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&path, perms);
        }
        let _ = std::fs::remove_dir_all(&dir);
        result.unwrap();
    }

    #[test]
    fn tray_icon_choice_mode_theme_connected() {
        use TileKind::*;
        let cases = [
            (Some(NoiseCancelling), true, false, "nc-dark.ico"),
            (Some(NoiseCancelling), true, true, "nc-light.ico"),
            (Some(Ambient), true, false, "ambient-dark.ico"),
            (Some(Ambient), true, true, "ambient-light.ico"),
            (Some(Off), true, false, "off-dark.ico"),
            (Some(Off), true, true, "off-light.ico"),
            (None, true, false, "off-dark.ico"),
            (Some(NoiseCancelling), false, false, "disconnected-dark.ico"),
            (Some(Ambient), false, true, "disconnected-light.ico"),
            (None, false, true, "disconnected-light.ico"),
        ];
        for (mode, connected, light, want) in cases {
            assert_eq!(
                tray_icon_choice(mode, connected, light),
                want,
                "{mode:?} {connected} {light}"
            );
        }
    }

    #[test]
    fn menu_marks_the_current_mode_and_sign_in_state() {
        let e = entries(&menu_items(Some(&connected(true)), true));
        let want = [
            ("nc", "Noise cancelling · current"),
            ("ambient", "Ambient"),
            ("off", "Off"),
            ("-", ""),
            ("open-at-sign-in", "Open at sign-in · On"),
            ("-", ""),
            ("quit", "Quit XM Reactor"),
        ];
        let want: Vec<(String, String)> = want
            .iter()
            .map(|(k, l)| (k.to_string(), l.to_string()))
            .collect();
        assert_eq!(e, want);
    }

    #[test]
    fn menu_hides_modes_when_disconnected() {
        for vm in [Some(disconnected()), None] {
            let e = entries(&menu_items(vm.as_ref(), false));
            let keys: Vec<&str> = e.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(keys, ["open-at-sign-in", "-", "quit"]);
            assert_eq!(e[0].1, "Open at sign-in · Off");
        }
    }

    #[test]
    fn menu_skips_hidden_tiles_and_greys_disabled_ones() {
        let mut vm = connected(true);
        fn tile(vm: &mut PanelVm, k: TileKind) -> &mut TileVm {
            vm.tiles.iter_mut().find(|t| t.kind == k).unwrap()
        }
        tile(&mut vm, TileKind::Ambient).visible = false;
        tile(&mut vm, TileKind::Off).enabled = false;
        let items = menu_items(Some(&vm), false);
        let keys: Vec<String> = entries(&items).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["nc", "off", "-", "open-at-sign-in", "-", "quit"]);
        assert!(matches!(items[1], MenuItem::Item { enabled: false, .. }));
    }

    #[test]
    fn menu_keys_map_to_actions() {
        let cases = [
            ("nc", TrayAction::Mode(TileKind::NoiseCancelling)),
            ("ambient", TrayAction::Mode(TileKind::Ambient)),
            ("off", TrayAction::Mode(TileKind::Off)),
            ("open-at-sign-in", TrayAction::ToggleOpenAtSignIn),
            ("quit", TrayAction::Quit),
        ];
        for (k, a) in cases {
            assert_eq!(menu_action(&Key::from(k)), Some(a));
        }
        assert_eq!(menu_action(&Key::from("sep-1")), None);
    }
}
