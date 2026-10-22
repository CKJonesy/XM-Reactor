//! Equaliser presets as chips on the mode tiles' three-column grid; the selected one is checked.
//! The XM6's nine presets make three full rows; a headset with another count leaves a short last
//! row rather than squeezing a label.

use super::panel::{Panel, PanelMsg};
use crate::strings;
use crate::view_model::{is_custom, EqVm};
use windows_reactor::*;
use xm_core::command::EqPreset;
use xm_core::log_debug;
use xm_core::model::Intent;

/// The same columns and gap as the mode tiles, so the chips line up under them.
const CHIP_COLUMNS: usize = 3;
const CHIP_GAP: f64 = 8.0;
const CHIP_HEIGHT: f64 = 36.0;

/// Row and column of the `i`th chip.
fn chip_cell(i: usize) -> (i32, i32) {
    ((i / CHIP_COLUMNS) as i32, (i % CHIP_COLUMNS) as i32)
}

/// What a tap on preset `p` asks of the headphones; None when the equaliser is paused or `p` is on.
pub fn preset_intent(eq: &EqVm, p: EqPreset) -> Option<Intent> {
    (eq.enabled && eq.selected != p).then_some(Intent::SetEqPreset(p))
}

/// Each chip's accessible name: its label, with " (custom)" on a custom preset whose nickname
/// matches another preset's name, so a screen reader never hears two identical chips.
pub fn chip_names(presets: &[(EqPreset, String)]) -> Vec<String> {
    presets
        .iter()
        .map(|(p, label)| {
            let clash = presets
                .iter()
                .any(|(q, other)| q != p && other.eq_ignore_ascii_case(label));
            if clash && is_custom(*p) {
                strings::custom_preset_name(label)
            } else {
                label.clone()
            }
        })
        .collect()
}

pub fn view(eq: &EqVm, cx: &ViewContext<'_, Panel>) -> View {
    let chips: Vec<View> = eq
        .presets
        .iter()
        .zip(chip_names(&eq.presets))
        .enumerate()
        .map(|(i, ((p, label), name))| {
            let p = *p;
            let (row, col) = chip_cell(i);
            // A nickname of up to 24 characters overflows the chip, so it trims and the tooltip
            // carries the full name. Built-in names fit with the stock padding.
            let text = TextBlock::new().text(label).font_size(12.0);
            let text = if is_custom(p) {
                text.max_lines(1)
                    .text_trimming(TextTrimming::CharacterEllipsis)
            } else {
                text
            };
            ToggleButton::new()
                .content(text)
                .is_checked(p == eq.selected)
                .is_enabled(eq.enabled)
                .automation_name(name)
                .horizontal_alignment(HorizontalAlignment::Stretch)
                .height(CHIP_HEIGHT)
                .grid_row(row)
                .grid_column(col)
                .on_is_checked_changed(cx.callback(move |v: Option<bool>| {
                    log_debug!("ui: chip {p:?} checked-changed {v:?}");
                    PanelMsg::Preset(p)
                }))
                .tooltip(label)
        })
        .collect();
    let mut rows: Vec<View> = vec![
        TextBlock::new()
            .text(strings::EQUALISER)
            .font_weight(FontWeight::SEMI_BOLD)
            .automation_heading_level(AutomationHeadingLevel::Level2)
            .into(),
        Grid::new()
            .columns([GridLength::STAR; CHIP_COLUMNS])
            .rows(vec![GridLength::Auto; chips.len().div_ceil(CHIP_COLUMNS)])
            .column_spacing(CHIP_GAP)
            .row_spacing(CHIP_GAP)
            .automation_name(strings::EQUALISER)
            .children(chips)
            .into(),
    ];
    if let Some(reason) = &eq.reason {
        rows.push(
            TextBlock::new()
                .text(reason)
                .font_size(12.0)
                .text_wrapping(TextWrapping::Wrap)
                .into(),
        );
    }
    StackPanel::new().spacing(4.0).children(rows).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eq(enabled: bool) -> EqVm {
        EqVm {
            presets: vec![
                (EqPreset::FLAT, "Flat".into()),
                (EqPreset::HEAVY, "Heavy".into()),
            ],
            selected: EqPreset::FLAT,
            enabled,
            reason: None,
        }
    }

    #[test]
    fn preset_tap() {
        assert_eq!(
            preset_intent(&eq(true), EqPreset::HEAVY),
            Some(Intent::SetEqPreset(EqPreset::HEAVY))
        );
        assert_eq!(preset_intent(&eq(true), EqPreset::FLAT), None);
        // Paused during a call: the tap is ignored.
        assert_eq!(preset_intent(&eq(false), EqPreset::HEAVY), None);
    }

    #[test]
    fn xm6_presets_make_three_full_rows_on_the_tile_grid() {
        use crate::view_model::{is_custom, EQ_PRESETS};
        assert_eq!(EQ_PRESETS.len() % CHIP_COLUMNS, 0);
        let rows: Vec<Vec<&str>> = EQ_PRESETS
            .chunks(CHIP_COLUMNS)
            .map(|r| r.iter().map(|(_, n)| *n).collect())
            .collect();
        assert_eq!(
            rows,
            [
                ["Flat", "Gaming", "Heavy"],
                ["Clear", "Hard", "Soft"],
                ["Manual", "Custom 1", "Custom 2"],
            ]
        );
        // The editable presets are exactly the last row.
        for (i, (p, _)) in EQ_PRESETS.iter().enumerate() {
            assert_eq!(is_custom(*p), chip_cell(i).0 == 2);
        }
        assert_eq!(chip_cell(0), (0, 0));
        assert_eq!(chip_cell(8), (2, 2));
    }

    #[test]
    fn no_chip_squeezes_a_label() {
        // 336 DIP of content (360 less the 12 DIP margins), three columns with two 8 DIP gaps,
        // less the stock 11 DIP padding each side.
        let room = (336.0 - 2.0 * CHIP_GAP) / CHIP_COLUMNS as f64 - 22.0;
        // 7 DIP a character at 12 px is above the widest measured ("Custom 2", 50.5 DIP).
        for (_, name) in crate::view_model::EQ_PRESETS {
            let width = 7.0 * name.chars().count() as f64;
            assert!(width <= room, "{name} needs {width} DIP of {room}");
        }
    }

    #[test]
    fn a_nickname_matching_another_preset_reads_as_custom() {
        let presets = vec![
            (EqPreset::HEAVY, "Heavy".to_string()),
            (EqPreset::CUSTOM1, "heavy".to_string()),
            (EqPreset::CUSTOM2, "Desk".to_string()),
        ];
        assert_eq!(chip_names(&presets), ["Heavy", "heavy (custom)", "Desk"]);
    }
}
