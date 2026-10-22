//! The quick-settings tiles (a "Noise control" row of exclusive modes over a row of independent
//! features) and the ambient controls, which stay in place and come alive in Ambient mode.

use super::panel::{Panel, PanelMsg};
use super::settings::switch_row;
use crate::strings;
use crate::view_model::{AmbientVm, TileKind, TileVm};
use windows_reactor::*;
use xm_core::command::NcAmbMode;
use xm_core::log_debug;
use xm_core::model::Intent;
use xm_core::settings::Settings;

const AMBIENT_MAX: u8 = 20;

/// Segoe Fluent Icons glyph for each tile. Ambient is the ear (hear what is around you) and Off is
/// the "none" sign (no noise processing); neither is a speaker or mute glyph, which would read
/// as cutting the audio.
pub fn tile_glyph(kind: TileKind) -> &'static str {
    match kind {
        TileKind::NoiseCancelling => "\u{E7F6}",
        TileKind::Ambient => "\u{F270}",
        TileKind::Off => "\u{E733}",
        TileKind::AutoPause => "\u{E769}",
        TileKind::SpeakToChat => "\u{E8BD}",
        TileKind::Dsee => "\u{E8D6}",
    }
}

/// Row and column; row 0 holds the three exclusive modes, row 1 the independent toggles.
fn cell(kind: TileKind) -> (i32, i32) {
    match kind {
        TileKind::NoiseCancelling => (0, 0),
        TileKind::Ambient => (0, 1),
        TileKind::Off => (0, 2),
        TileKind::AutoPause => (1, 0),
        TileKind::SpeakToChat => (1, 1),
        TileKind::Dsee => (1, 2),
    }
}

/// What a tap on `tile` asks of the headphones; None when the tile is not live or nothing changes.
pub fn tile_intent(tile: &TileVm, connected: bool) -> Option<Intent> {
    if !(connected && tile.visible && tile.enabled) {
        return None;
    }
    let mode = |m| (!tile.on).then_some(Intent::SetMode(m));
    match tile.kind {
        TileKind::NoiseCancelling => mode(NcAmbMode::NoiseCancelling),
        TileKind::Ambient => mode(NcAmbMode::Ambient),
        TileKind::Off => mode(NcAmbMode::Off),
        TileKind::AutoPause => Some(Intent::SetAutoPause(!tile.on)),
        TileKind::SpeakToChat => Some(Intent::SetSpeakToChat(!tile.on)),
        TileKind::Dsee => Some(Intent::SetDsee(!tile.on)),
    }
}

/// What the Focus on voice switch asks for; None when the headphones already match.
pub fn focus_intent(on: bool, ambient: &AmbientVm) -> Option<Intent> {
    (ambient.enabled && on != ambient.focus_on_voice).then_some(Intent::SetFocusOnVoice(on))
}

/// The level a slider position asks for; None when it matches the headphones already.
pub fn ambient_intent(value: f64, ambient: &AmbientVm) -> Option<Intent> {
    let level = value.round().clamp(0.0, f64::from(AMBIENT_MAX)) as u8;
    (ambient.enabled && level != ambient.level).then_some(Intent::SetAmbientLevel(level))
}

/// The app's own pause and resume of Windows media follow the headphones' auto-pause, so the one
/// tile tells the truth about both (Ruling 23). None when the settings already match.
pub fn auto_pause_prefs(settings: &Settings, on: bool) -> Option<Settings> {
    let next = Settings {
        pause_on_removal: on,
        resume_on_wear: on,
        ..settings.clone()
    };
    (next != *settings).then_some(next)
}

/// What one tap on `tile` does: the intent for the headphones, and for "Pause when taken off" the
/// settings to save as well. None when the tile is not live or nothing changes.
pub fn tile_tap(
    tile: &TileVm,
    connected: bool,
    settings: &Settings,
) -> Option<(Intent, Option<Settings>)> {
    let intent = tile_intent(tile, connected)?;
    let save = match intent {
        Intent::SetAutoPause(on) => auto_pause_prefs(settings, on),
        _ => None,
    };
    Some((intent, save))
}

fn tile(t: &TileVm, cx: &ViewContext<'_, Panel>) -> View {
    let mut lines: Vec<View> = vec![
        FontIcon::new()
            .glyph(tile_glyph(t.kind))
            .font_size(16.0)
            .into(),
        TextBlock::new()
            .text(&t.label)
            .font_size(12.0)
            .text_alignment(TextAlignment::Center)
            .text_wrapping(TextWrapping::Wrap)
            .max_lines(2)
            .into(),
    ];
    if let Some(sub) = &t.subtitle {
        lines.push(
            TextBlock::new()
                .text(sub)
                .font_size(11.0)
                .text_alignment(TextAlignment::Center)
                .text_wrapping(TextWrapping::Wrap)
                .max_lines(2)
                .into(),
        );
    }
    let name = strings::tile_name(&t.label, t.subtitle.as_deref());
    let (_, col) = cell(t.kind);
    let kind = t.kind;
    ToggleButton::new()
        .content(StackPanel::new().spacing(4.0).children(lines))
        .is_checked(t.on)
        .is_enabled(t.enabled)
        .automation_name(name)
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .vertical_alignment(VerticalAlignment::Stretch)
        .min_height(72.0)
        .grid_column(col)
        .on_is_checked_changed(cx.callback(move |v: Option<bool>| {
            log_debug!("ui: tile {kind:?} checked-changed {v:?}");
            PanelMsg::Tile(kind)
        }))
        .tooltip(t.tooltip)
}

/// One row of three cells; a hidden tile leaves its cell empty.
fn row(tiles: &[TileVm], row: i32, cx: &ViewContext<'_, Panel>) -> Grid {
    Grid::new()
        .columns([GridLength::STAR; 3])
        .column_spacing(8.0)
        .children(
            tiles
                .iter()
                .filter(|t| t.visible && cell(t.kind).0 == row)
                .map(|t| tile(t, cx))
                .collect::<Vec<View>>(),
        )
}

pub fn view(tiles: &[TileVm], cx: &ViewContext<'_, Panel>) -> View {
    StackPanel::new()
        .spacing(4.0)
        .children((
            TextBlock::new()
                .text(strings::NOISE_CONTROL)
                .font_weight(FontWeight::SEMI_BOLD)
                .automation_heading_level(AutomationHeadingLevel::Level2),
            row(tiles, 0, cx).automation_name(strings::NOISE_CONTROL),
            // The extra gap tells the exclusive modes from the independent features below.
            row(tiles, 1, cx).margin(Thickness::new(0.0, 12.0, 0.0, 0.0)),
        ))
        .into()
}

/// Mounted only in Ambient mode (see `panel`).
pub fn ambient_view(a: &AmbientVm, connected: bool, cx: &ViewContext<'_, Panel>) -> View {
    StackPanel::new()
        .spacing(4.0)
        .children((
            Slider::new()
                .header(strings::ambient_level_header(a.level).as_str())
                .minimum(0.0)
                .maximum(f64::from(AMBIENT_MAX))
                .step_frequency(1.0)
                .value(f64::from(a.level))
                .is_enabled(connected)
                .automation_name(strings::ambient_level_name(a.level, AMBIENT_MAX))
                .on_value_changed(cx.callback(PanelMsg::Ambient)),
            switch_row(
                TextBlock::new().text(strings::FOCUS_ON_VOICE),
                ToggleSwitch::new()
                    .is_on(a.focus_on_voice)
                    .is_enabled(connected)
                    .automation_name(strings::FOCUS_ON_VOICE)
                    .on_toggled(cx.callback(PanelMsg::FocusOnVoice)),
            ),
        ))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tv(kind: TileKind, on: bool) -> TileVm {
        TileVm {
            kind,
            label: String::new(),
            subtitle: None,
            visible: true,
            enabled: true,
            on,
            tooltip: "",
        }
    }

    #[test]
    fn auto_pause_tile_drives_the_app_prefs_too() {
        let s = Settings::default();
        assert!(s.pause_on_removal && s.resume_on_wear);
        assert_eq!(
            tile_intent(&tv(TileKind::AutoPause, true), true),
            Some(Intent::SetAutoPause(false))
        );
        let off = auto_pause_prefs(&s, false).unwrap();
        assert!(!off.pause_on_removal && !off.resume_on_wear);
        assert_eq!(off.eq_names, s.eq_names);
        // Already matching: no needless save.
        assert_eq!(auto_pause_prefs(&off, false), None);
        let on = auto_pause_prefs(&off, true).unwrap();
        assert!(on.pause_on_removal && on.resume_on_wear);
        // One half off on its own (an old settings file) is put right.
        let half = Settings {
            resume_on_wear: false,
            ..s
        };
        assert!(auto_pause_prefs(&half, true).unwrap().resume_on_wear);
    }

    #[test]
    fn one_tap_is_one_intent_and_at_most_one_save() {
        let s = Settings::default();
        let (intent, save) = tile_tap(&tv(TileKind::AutoPause, true), true, &s).unwrap();
        assert_eq!(intent, Intent::SetAutoPause(false));
        let save = save.unwrap();
        assert!(!save.pause_on_removal && !save.resume_on_wear);
        // Prefs already matching the new state: the intent goes out, nothing is saved.
        assert_eq!(
            tile_tap(&tv(TileKind::AutoPause, false), true, &s),
            Some((Intent::SetAutoPause(true), None))
        );
        // Other tiles never save; a tap while disconnected does nothing at all.
        assert_eq!(
            tile_tap(&tv(TileKind::Dsee, false), true, &s),
            Some((Intent::SetDsee(true), None))
        );
        assert_eq!(tile_tap(&tv(TileKind::AutoPause, true), false, &s), None);
    }

    #[test]
    fn glyphs_and_cells_are_distinct() {
        let kinds = [
            TileKind::NoiseCancelling,
            TileKind::Ambient,
            TileKind::Off,
            TileKind::AutoPause,
            TileKind::SpeakToChat,
            TileKind::Dsee,
        ];
        for a in kinds {
            assert_eq!(tile_glyph(a).chars().count(), 1);
            for b in kinds {
                assert_eq!(a == b, tile_glyph(a) == tile_glyph(b));
                assert_eq!(a == b, cell(a) == cell(b));
            }
        }
    }

    #[test]
    fn tap_maps_to_intent() {
        assert_eq!(
            tile_intent(&tv(TileKind::Ambient, false), true),
            Some(Intent::SetMode(NcAmbMode::Ambient))
        );
        // Tapping the mode already on changes nothing.
        assert_eq!(
            tile_intent(&tv(TileKind::NoiseCancelling, true), true),
            None
        );
        assert_eq!(
            tile_intent(&tv(TileKind::Dsee, true), true),
            Some(Intent::SetDsee(false))
        );
        assert_eq!(
            tile_intent(&tv(TileKind::AutoPause, false), true),
            Some(Intent::SetAutoPause(true))
        );
    }

    #[test]
    fn tap_ignored_when_disconnected_or_disabled() {
        assert_eq!(tile_intent(&tv(TileKind::Off, false), false), None);
        let mut t = tv(TileKind::SpeakToChat, false);
        t.enabled = false;
        assert_eq!(tile_intent(&t, true), None);
    }

    #[test]
    fn focus_on_voice_sends_only_a_change() {
        let a = AmbientVm {
            level: 10,
            focus_on_voice: true,
            enabled: true,
        };
        assert_eq!(focus_intent(true, &a), None);
        assert_eq!(
            focus_intent(false, &a),
            Some(Intent::SetFocusOnVoice(false))
        );
    }

    #[test]
    fn tile_name_includes_subtitle() {
        assert_eq!(strings::tile_name("Off", None), "Off");
        assert_eq!(
            strings::tile_name("Noise cancelling", Some("Paused while off your head")),
            "Noise cancelling, Paused while off your head"
        );
    }

    #[test]
    fn ambient_level_rounds_clamps_and_skips_unchanged() {
        let a = AmbientVm {
            level: 10,
            focus_on_voice: false,
            enabled: true,
        };
        assert_eq!(ambient_intent(10.2, &a), None);
        assert_eq!(ambient_intent(12.6, &a), Some(Intent::SetAmbientLevel(13)));
        assert_eq!(ambient_intent(99.0, &a), Some(Intent::SetAmbientLevel(20)));
        assert_eq!(ambient_intent(-3.0, &a), Some(Intent::SetAmbientLevel(0)));
        // Outside Ambient mode the row is shown but sends nothing.
        let idle = AmbientVm {
            enabled: false,
            ..a
        };
        assert_eq!(ambient_intent(15.0, &idle), None);
        assert_eq!(focus_intent(true, &idle), None);
    }
}
