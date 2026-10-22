//! Listening mode on the main view: Standard, Background music and Cinema as chips on the same
//! three-column grid as the presets, with the room chips under them while Background music is on.
//! Taps go through the settings intents, which turn the old mode off before the new one on.

use super::panel::{Panel, PanelMsg};
use super::settings::SettingsMsg;
use crate::strings;
use crate::view_model::{ListeningMode, SettingsVm, ROOMS};
use windows_reactor::*;

const COLUMNS: usize = 3;
const GAP: f64 = 8.0;
const CHIP_HEIGHT: f64 = 36.0;

/// One row of exclusive chips; the selected one is checked.
fn chips(
    labels: &[&str],
    selected: Option<usize>,
    name: &str,
    msg: fn(Option<usize>) -> SettingsMsg,
    cx: &ViewContext<'_, Panel>,
) -> View {
    let chips: Vec<View> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            // "Background music" needs two lines at the chip width; the row grows to match.
            ToggleButton::new()
                .content(
                    TextBlock::new()
                        .text(*label)
                        .font_size(12.0)
                        .text_alignment(TextAlignment::Center)
                        .text_wrapping(TextWrapping::Wrap)
                        .max_lines(2),
                )
                .is_checked(selected == Some(i))
                .automation_name(*label)
                .horizontal_alignment(HorizontalAlignment::Stretch)
                .vertical_alignment(VerticalAlignment::Stretch)
                .min_height(CHIP_HEIGHT)
                .grid_row((i / COLUMNS) as i32)
                .grid_column((i % COLUMNS) as i32)
                .on_is_checked_changed(
                    cx.callback(move |_: Option<bool>| PanelMsg::Settings(msg(Some(i)))),
                )
                .tooltip(*label)
                .into()
        })
        .collect();
    Grid::new()
        .columns([GridLength::STAR; COLUMNS])
        .rows(vec![GridLength::Auto; chips.len().div_ceil(COLUMNS)])
        .column_spacing(GAP)
        .row_spacing(GAP)
        .automation_name(name)
        .children(chips)
        .into()
}

/// Mounted only while the headphones offer a listening mode (see `panel`).
pub fn view(s: &SettingsVm, cx: &ViewContext<'_, Panel>) -> View {
    let labels: Vec<&str> = s.listening_modes.iter().map(|(_, l)| l.as_str()).collect();
    let selected = s
        .listening_modes
        .iter()
        .position(|(m, _)| Some(*m) == s.listening_mode);
    let mut rows: Vec<View> = vec![
        TextBlock::new()
            .text(strings::LISTENING_MODE)
            .font_weight(FontWeight::SEMI_BOLD)
            .automation_heading_level(AutomationHeadingLevel::Level2)
            .into(),
        chips(
            &labels,
            selected,
            strings::LISTENING_MODE,
            SettingsMsg::ListeningMode,
            cx,
        ),
    ];
    if s.listening_mode == Some(ListeningMode::Bgm) {
        let rooms: Vec<&str> = ROOMS.iter().map(|(_, l)| *l).collect();
        let room = ROOMS.iter().position(|(r, _)| s.room == Some(*r));
        rows.push(chips(&rooms, room, strings::ROOM, SettingsMsg::Room, cx));
    }
    StackPanel::new().spacing(4.0).children(rows).into()
}
