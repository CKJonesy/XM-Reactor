//! The band editor, mounted on the main view under the preset chips once the bands are known; the
//! flyout takes its tall height meanwhile (Ruling 29). It holds the preset's name, the equaliser
//! curve and Clear Bass on older models. A custom preset (Manual, Custom 1 or Custom 2) adds Rename
//! and Reset to flat and can be edited; a built-in preset's curve is read-only. The model debounces band writes, so every change can go straight out as an
//! intent.

use super::eq_curve::{self, Curve};
use super::panel::{Panel, PanelMsg};
use crate::strings;
use crate::view_model::{eq_name_key, is_custom, preset_name, SettingsVm};
use windows_reactor::*;
use xm_core::command::EqPreset;
use xm_core::model::Intent;
use xm_core::settings::Settings;

/// The longest nickname kept; the Rename box is cut to it as you type (reactor has no MaxLength).
pub const NAME_MAX: usize = 24;

/// The settings after renaming custom preset `p` in the app; a blank name, or the preset's own
/// name, restores the original. None when `p` is fixed or nothing changes.
pub fn rename(settings: &Settings, p: EqPreset, text: &str) -> Option<Settings> {
    if !is_custom(p) {
        return None;
    }
    let name: String = text.trim().chars().take(NAME_MAX).collect();
    let mut next = settings.clone();
    match name.trim_end() {
        n if n.is_empty() || Some(n) == preset_name(p) => next.eq_names.remove(&eq_name_key(p)),
        n => next.eq_names.insert(eq_name_key(p), n.to_string()),
    };
    (next != *settings).then_some(next)
}

fn level(value: f64, max: i8) -> i8 {
    value.round().clamp(f64::from(-max), f64::from(max)) as i8
}

/// What moving band `band` to `value` asks for; None when the bands are locked or nothing changes.
pub fn band_intent(s: &SettingsVm, band: usize, value: f64) -> Option<Intent> {
    let current = *s.bands.get(band).filter(|_| s.bands_enabled)?;
    let l = level(value, s.band_max);
    (l != current).then(|| {
        let mut bands = s.bands.clone();
        bands[band] = l;
        Intent::SetEqBands(s.clear_bass, bands)
    })
}

pub fn clear_bass_intent(s: &SettingsVm, value: f64) -> Option<Intent> {
    let current = s.clear_bass.filter(|_| s.bands_enabled)?;
    let l = level(value, s.band_max);
    (l != current).then(|| Intent::SetEqBands(Some(l), s.bands.clone()))
}

pub fn reset_intent(s: &SettingsVm) -> Option<Intent> {
    let flat = s.bands.iter().all(|b| *b == 0) && s.clear_bass.unwrap_or(0) == 0;
    (s.bands_enabled && !flat)
        .then(|| Intent::SetEqBands(s.clear_bass.map(|_| 0), vec![0; s.bands.len()]))
}

/// `name` is the selected preset's label, which may be the user's nickname. The editor has no
/// caption of its own: during a call or a listening mode the chips' reason line above says why the
/// bands are locked.
/// The selected chip already names the preset, so the row above the curve carries only what the
/// chip cannot: Rename and Reset to flat for a custom preset, a caption for a built-in one.
pub fn view(s: &SettingsVm, curve: &Curve, cx: &ViewContext<'_, Panel>) -> View {
    let max = f64::from(s.band_max);
    let mut header: Vec<View> = Vec::new();
    if s.custom_preset {
        header.push(
            Button::new()
                .content(strings::RENAME)
                .automation_name(strings::RENAME_PRESET)
                .grid_column(1)
                .on_click(cx.message(PanelMsg::RenameOpen))
                .into(),
        );
        header.push(
            Button::new()
                .content(strings::RESET_FLAT)
                .is_enabled(reset_intent(s).is_some())
                .automation_name(strings::RESET_FLAT)
                .grid_column(2)
                .on_click(cx.message(PanelMsg::ResetFlat))
                .into(),
        );
    } else {
        header.push(
            TextBlock::new()
                .text(strings::EQ_BUILT_IN_CAPTION)
                .font_size(12.0)
                .vertical_alignment(VerticalAlignment::Center)
                .into(),
        );
    }
    let mut rows: Vec<View> = vec![
        // One button-height tall either way, so the curve stays put between presets.
        Grid::new()
            .columns([GridLength::STAR, GridLength::Auto, GridLength::Auto])
            .column_spacing(8.0)
            .min_height(32.0)
            .children(header)
            .into(),
        eq_curve::view(s, curve, cx),
    ];
    if let Some(cb) = s.clear_bass {
        rows.push(
            Slider::new()
                .header(strings::CLEAR_BASS)
                .minimum(-max)
                .maximum(max)
                .step_frequency(1.0)
                .value(f64::from(cb))
                .is_enabled(s.bands_enabled)
                .automation_name(strings::CLEAR_BASS)
                .on_value_changed(cx.callback(PanelMsg::ClearBass))
                .into(),
        );
    }
    StackPanel::new().spacing(8.0).children(rows).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm(enabled: bool, clear_bass: Option<i8>, bands: Vec<i8>) -> SettingsVm {
        SettingsVm {
            clear_bass,
            bands,
            band_max: 6,
            bands_enabled: enabled,
            ..SettingsVm::default()
        }
    }

    #[test]
    fn band_names_read_with_units() {
        assert_eq!(strings::band_name("31"), "31 Hz band");
        assert_eq!(strings::band_name("1k"), "1 kHz band");
        assert_eq!(strings::band_name("2.5k"), "2.5 kHz band");
        assert_eq!(strings::band_value_name("63", 3), "63 Hz band, +3 dB");
        assert_eq!(strings::band_value_name("1k", -6), "1 kHz band, -6 dB");
        assert_eq!(strings::band_value_name("16k", 0), "16 kHz band, 0 dB");
    }

    #[test]
    fn rename_trims_caps_and_restores() {
        let s = Settings::default();
        let named = rename(&s, EqPreset::CUSTOM1, "  Desk  ").unwrap();
        assert_eq!(named.eq_names.get("A1").map(String::as_str), Some("Desk"));
        let long = rename(&s, EqPreset::MANUAL, &"x".repeat(30)).unwrap();
        assert_eq!(long.eq_names["A0"].chars().count(), 24);
        // Empty or blank restores the headphones' name.
        let back = rename(&named, EqPreset::CUSTOM1, "   ").unwrap();
        assert!(back.eq_names.is_empty());
        assert_eq!(rename(&named, EqPreset::CUSTOM1, "Desk"), None);
        assert_eq!(rename(&s, EqPreset::CUSTOM1, ""), None);
        // Keeping the original name stores nothing, and typing it back clears a nickname.
        assert_eq!(rename(&s, EqPreset::MANUAL, " Manual "), None);
        let back = rename(&named, EqPreset::CUSTOM1, "Custom 1").unwrap();
        assert!(back.eq_names.is_empty());
        // Fixed presets cannot be renamed.
        assert_eq!(rename(&s, EqPreset::GAMING, "Mine"), None);
    }

    #[test]
    fn band_move_rounds_clamps_and_skips_unchanged() {
        let s = vm(true, None, vec![0, 2, 0]);
        assert_eq!(band_intent(&s, 1, 2.3), None);
        assert_eq!(
            band_intent(&s, 1, 9.0),
            Some(Intent::SetEqBands(None, vec![0, 6, 0]))
        );
        assert_eq!(
            band_intent(&s, 0, -2.6),
            Some(Intent::SetEqBands(None, vec![-3, 2, 0]))
        );
        assert_eq!(band_intent(&s, 7, 1.0), None);
        assert_eq!(band_intent(&vm(false, None, vec![0, 2, 0]), 0, 1.0), None);
    }

    #[test]
    fn clear_bass_keeps_the_bands() {
        let s = vm(true, Some(0), vec![1, 1]);
        assert_eq!(
            clear_bass_intent(&s, 4.0),
            Some(Intent::SetEqBands(Some(4), vec![1, 1]))
        );
        assert_eq!(clear_bass_intent(&s, 0.2), None);
        assert_eq!(clear_bass_intent(&vm(true, None, vec![1]), 4.0), None);
    }

    #[test]
    fn reset_only_when_not_flat() {
        assert_eq!(reset_intent(&vm(true, None, vec![0, 0])), None);
        assert_eq!(
            reset_intent(&vm(true, Some(3), vec![0, 0])),
            Some(Intent::SetEqBands(Some(0), vec![0, 0]))
        );
        assert_eq!(reset_intent(&vm(false, None, vec![2, 0])), None);
    }
}
