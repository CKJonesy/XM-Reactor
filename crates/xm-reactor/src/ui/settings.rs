//! The settings page, shown in place of the main view: a selector bar with Headphones, App and
//! About, one view at a time so each fits the flyout without scrolling. Listening mode lives on
//! the main view; its intents are still built here. It renders inside the panel, which owns the
//! view model and the engine handle.

use super::panel::{Panel, PanelMsg};
use crate::strings;
use crate::view_model::{InfoRow, ListeningMode, PanelVm, SettingsVm, SourceVm, ROOMS};
use windows_reactor::*;
use xm_core::command::BgmRoom;
use xm_core::model::Intent;
use xm_core::settings::{HeadsetColour, Settings, ThemeChoice};

#[derive(Debug, Clone)]
pub enum SettingsMsg {
    ListeningMode(Option<usize>),
    Room(Option<usize>),
    AutoPowerOff(Option<usize>),
    Sidetone(bool),
    PlayHere(String),
    Colour(Option<usize>),
    BatteryInTooltip(bool),
    LowBattery(bool),
    OpenAtSignIn(bool),
    Theme(Option<usize>),
    OpenLogFolder,
}

const COLOURS: [(Option<HeadsetColour>, &str); 4] = [
    (None, strings::COLOUR_AUTO),
    (Some(HeadsetColour::PlatinumSilver), strings::COLOUR_SILVER),
    (Some(HeadsetColour::MidnightBlue), strings::COLOUR_BLUE),
    (Some(HeadsetColour::Black), strings::COLOUR_BLACK),
];

const THEMES: [(ThemeChoice, &str); 3] = [
    (ThemeChoice::FollowWindows, strings::THEME_FOLLOW),
    (ThemeChoice::Light, strings::THEME_LIGHT),
    (ThemeChoice::Dark, strings::THEME_DARK),
];

/// The page's three views; the panel remembers which is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Headphones,
    App,
    About,
}

pub const TABS: [SettingsTab; 3] = [
    SettingsTab::Headphones,
    SettingsTab::App,
    SettingsTab::About,
];

impl SettingsTab {
    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Headphones => strings::SECTION_HEADPHONES,
            SettingsTab::App => strings::SECTION_APP,
            SettingsTab::About => strings::SECTION_ABOUT,
        }
    }
}

/// The tab whose label the selector bar reported; None for a cleared or unknown selection.
pub fn tab_from_text(text: Option<&str>) -> Option<SettingsTab> {
    TABS.into_iter().find(|t| Some(t.label()) == text)
}

/// Turns the current listening mode off before turning the new one on, so two are never on.
fn listening_intents(s: &SettingsVm, to: ListeningMode) -> Vec<Intent> {
    let Some(from) = s.listening_mode.filter(|m| *m != to) else {
        return Vec::new();
    };
    let room = s.room.unwrap_or(BgmRoom::MyRoom);
    let mut out = Vec::new();
    match from {
        ListeningMode::Bgm => out.push(Intent::SetBgm(false, room)),
        ListeningMode::Cinema => out.push(Intent::SetCinema(false)),
        ListeningMode::Standard => {}
    }
    match to {
        ListeningMode::Bgm => out.push(Intent::SetBgm(true, room)),
        ListeningMode::Cinema => out.push(Intent::SetCinema(true)),
        ListeningMode::Standard => {}
    }
    out
}

/// What a settings change asks of the headphones, in order; empty when nothing changes.
pub fn intents(msg: &SettingsMsg, s: &SettingsVm) -> Vec<Intent> {
    let one = match msg {
        SettingsMsg::ListeningMode(Some(i)) => {
            return s
                .listening_modes
                .get(*i)
                .map_or_else(Vec::new, |(m, _)| listening_intents(s, *m));
        }
        SettingsMsg::Room(Some(i)) => ROOMS
            .get(*i)
            .filter(|(r, _)| s.listening_mode == Some(ListeningMode::Bgm) && s.room != Some(*r))
            .map(|(r, _)| Intent::SetBgm(true, *r)),
        SettingsMsg::AutoPowerOff(Some(i)) => s
            .auto_power_off
            .get(*i)
            .filter(|(a, _)| s.auto_power_off_selected != Some(*a))
            .map(|(a, _)| Intent::SetAutoPowerOff(*a)),
        SettingsMsg::Sidetone(on) => s
            .sidetone
            .filter(|(_, cur)| cur != on)
            .map(|(slot, _)| Intent::SetGeneralSetting(slot, *on)),
        SettingsMsg::PlayHere(mac) => s
            .sources
            .iter()
            .find(|d| d.mac == *mac && d.can_switch)
            .map(|d| Intent::SwitchSource(d.mac.clone())),
        _ => None,
    };
    one.into_iter().collect()
}

/// The saved settings after an app preference changes; None when nothing changes.
pub fn app_change(msg: &SettingsMsg, current: &Settings) -> Option<Settings> {
    let mut next = current.clone();
    match msg {
        SettingsMsg::Colour(Some(i)) => next.headset_colour = COLOURS.get(*i)?.0,
        SettingsMsg::BatteryInTooltip(on) => next.battery_in_tooltip = *on,
        SettingsMsg::LowBattery(on) => next.low_battery_notifications = *on,
        SettingsMsg::Theme(Some(i)) => next.theme = THEMES.get(*i)?.0,
        _ => return None,
    }
    (next != *current).then_some(next)
}

fn msg(m: SettingsMsg) -> PanelMsg {
    PanelMsg::Settings(m)
}

fn labels<T>(items: &[(T, &'static str)]) -> Vec<&'static str> {
    items.iter().map(|(_, l)| *l).collect()
}

/// A row's label with its one-line description underneath, Windows Settings style.
fn described(label: &str, description: Option<&str>) -> View {
    let mut lines: Vec<View> = vec![TextBlock::new()
        .text(label)
        .text_wrapping(TextWrapping::Wrap)
        .into()];
    if let Some(d) = description {
        lines.push(
            TextBlock::new()
                .text(d)
                .font_size(12.0)
                .text_wrapping(TextWrapping::Wrap)
                .into(),
        );
    }
    StackPanel::new().spacing(2.0).children(lines).into()
}

fn combo(label: &str, description: &str, items: Vec<&str>, selected: Option<usize>) -> ComboBox {
    ComboBox::new()
        .header(described(label, Some(description)))
        .items_source(items)
        .selected_index(selected)
        .automation_name(label)
        .horizontal_alignment(HorizontalAlignment::Stretch)
}

/// The label on the left and the switch on the right of the same row, as in Windows Settings.
pub fn switch_row(label: impl Into<View>, switch: ToggleSwitch) -> View {
    Grid::new()
        .columns([GridLength::STAR, GridLength::Auto])
        .column_spacing(12.0)
        .children((
            Border::new()
                .vertical_alignment(VerticalAlignment::Center)
                .content(label),
            // Sized to the knob and its On/Off text rather than the stock 154 DIP minimum.
            switch
                .min_width(0.0)
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(1),
        ))
        .into()
}

fn switch(label: &str, description: &str, on: bool, toggled: Callback<bool>) -> View {
    switch_row(
        described(label, Some(description)),
        ToggleSwitch::new()
            .is_on(on)
            .automation_name(label)
            .on_toggled(toggled),
    )
}

fn text(t: &str) -> View {
    TextBlock::new()
        .text(t)
        .text_wrapping(TextWrapping::Wrap)
        .into()
}

/// Label and value on one line, the description underneath across the full width.
fn info_row(label: &str, description: Option<&str>, value: &str) -> View {
    let mut cells: Vec<View> = vec![
        TextBlock::new()
            .text(label)
            .text_wrapping(TextWrapping::Wrap)
            .into(),
        TextBlock::new()
            .text(value)
            .text_wrapping(TextWrapping::Wrap)
            .text_alignment(TextAlignment::Right)
            .grid_column(1)
            .into(),
    ];
    if let Some(d) = description {
        cells.push(
            TextBlock::new()
                .text(d)
                .font_size(12.0)
                .text_wrapping(TextWrapping::Wrap)
                .grid_row(1)
                .grid_column_span(2)
                .into(),
        );
    }
    // The label keeps its width; a long value ("Noise cancelling ↔ Ambient") wraps on the right.
    Grid::new()
        .columns([GridLength::Auto, GridLength::STAR])
        .rows([GridLength::Auto; 2])
        .column_spacing(12.0)
        .row_spacing(2.0)
        .children(cells)
        .into()
}

fn source_row(d: &SourceVm, cx: &ViewContext<'_, Panel>) -> View {
    let mut cells: Vec<View> = vec![StackPanel::new()
        .children((
            TextBlock::new()
                .text(&d.name)
                .max_lines(1)
                .text_trimming(TextTrimming::CharacterEllipsis),
            TextBlock::new().text(&d.status).font_size(12.0),
        ))
        .into()];
    if d.can_switch {
        cells.push(
            Button::new()
                .content(strings::PLAY_HERE)
                .automation_name(strings::play_here_name(&d.name))
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(1)
                .on_click(cx.message(msg(SettingsMsg::PlayHere(d.mac.clone()))))
                .into(),
        );
    }
    Grid::new()
        .columns([GridLength::STAR, GridLength::Auto])
        .column_spacing(12.0)
        .children(cells)
        .into()
}

/// A Windows Settings-style card of rows.
fn card(children: Vec<View>) -> View {
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(Thickness::uniform(1.0))
        .corner_radius(CornerRadius::uniform(8.0))
        .padding(Thickness::uniform(12.0))
        .content(StackPanel::new().spacing(12.0).children(children))
        .into()
}

/// A second group on a tab: a heading over its card. The first card needs none, the tab names it.
fn section(title: &str, children: Vec<View>) -> View {
    StackPanel::new()
        .spacing(6.0)
        .children((
            TextBlock::new()
                .text(title)
                .font_weight(FontWeight::SEMI_BOLD)
                .automation_heading_level(AutomationHeadingLevel::Level2),
            card(children),
        ))
        .into()
}

/// The stock selector bar under the title: Headphones, App, About.
fn selector(tab: SettingsTab, cx: &ViewContext<'_, Panel>) -> View {
    SelectorBar::new()
        .on_selected_text_changed(cx.callback(|t: Option<std::rc::Rc<str>>| {
            PanelMsg::SettingsTab(tab_from_text(t.as_deref()))
        }))
        .keyed_items(TABS.iter().map(|t| {
            KeyedView::new(
                t.label(),
                SelectorBarItem::new()
                    .text(t.label())
                    .is_selected(*t == tab),
            )
        }))
        .into()
}

fn headphones(s: &SettingsVm, cx: &ViewContext<'_, Panel>) -> Vec<View> {
    let mut out = Vec::new();
    if !s.auto_power_off.is_empty() {
        let items: Vec<&str> = s.auto_power_off.iter().map(|(_, l)| l.as_str()).collect();
        let selected = s
            .auto_power_off
            .iter()
            .position(|(a, _)| s.auto_power_off_selected == Some(*a));
        out.push(
            combo(
                strings::AUTO_POWER_OFF,
                strings::AUTO_POWER_OFF_DESC,
                items,
                selected,
            )
            .on_selection_changed(cx.callback(|i| msg(SettingsMsg::AutoPowerOff(i))))
            .into(),
        );
    }
    if let Some((_, on)) = s.sidetone {
        out.push(switch(
            strings::CAPTURE_VOICE,
            strings::CAPTURE_VOICE_DESC,
            on,
            cx.callback(|on| msg(SettingsMsg::Sidetone(on))),
        ));
    }
    if !s.sources.is_empty() {
        out.push(
            StackPanel::new()
                .spacing(2.0)
                .children((
                    TextBlock::new()
                        .text(strings::CONNECTED_DEVICES)
                        .font_weight(FontWeight::SEMI_BOLD)
                        .automation_heading_level(AutomationHeadingLevel::Level3),
                    TextBlock::new()
                        .text(strings::CONNECTED_DEVICES_DESC)
                        .font_size(12.0)
                        .text_wrapping(TextWrapping::Wrap),
                ))
                .into(),
        );
        out.extend(s.sources.iter().map(|d| source_row(d, cx)));
    }
    out
}

/// The headset's own settings the app can only read; the Bluetooth address goes under About.
fn on_headphones(s: &SettingsVm) -> Vec<View> {
    let rows: Vec<&InfoRow> = s
        .info
        .iter()
        .filter(|r| r.label != strings::INFO_ADDRESS)
        .collect();
    if rows.is_empty() {
        return Vec::new();
    }
    // Read-only rows would otherwise look like settings that are broken.
    let mut out: Vec<View> = vec![TextBlock::new()
        .text(strings::READ_ONLY_NOTE)
        .font_size(12.0)
        .text_wrapping(TextWrapping::Wrap)
        .into()];
    out.extend(
        rows.into_iter()
            .map(|r| info_row(&r.label, r.description, &r.value)),
    );
    out
}

fn app(s: &SettingsVm, open_at_sign_in: bool, cx: &ViewContext<'_, Panel>) -> Vec<View> {
    let a = &s.app;
    let colour = COLOURS.iter().position(|(c, _)| *c == a.headset_colour);
    let theme = THEMES.iter().position(|(t, _)| *t == a.theme);
    vec![
        combo(
            strings::HEADSET_COLOUR,
            strings::HEADSET_COLOUR_DESC,
            labels(&COLOURS),
            colour,
        )
        .on_selection_changed(cx.callback(|i| msg(SettingsMsg::Colour(i))))
        .into(),
        switch(
            strings::BATTERY_IN_TOOLTIP,
            strings::BATTERY_IN_TOOLTIP_DESC,
            a.battery_in_tooltip,
            cx.callback(|on| msg(SettingsMsg::BatteryInTooltip(on))),
        ),
        switch(
            strings::LOW_BATTERY_NOTIFICATION,
            strings::LOW_BATTERY_DESC,
            a.low_battery_notifications,
            cx.callback(|on| msg(SettingsMsg::LowBattery(on))),
        ),
        switch(
            strings::OPEN_AT_SIGN_IN,
            strings::OPEN_AT_SIGN_IN_DESC,
            open_at_sign_in,
            cx.callback(|on| msg(SettingsMsg::OpenAtSignIn(on))),
        ),
        combo(strings::THEME, strings::THEME_DESC, labels(&THEMES), theme)
            .on_selection_changed(cx.callback(|i| msg(SettingsMsg::Theme(i))))
            .into(),
    ]
}

fn about(vm: &PanelVm, cx: &ViewContext<'_, Panel>) -> Vec<View> {
    let s = &vm.settings;
    let or_unknown = |v: &Option<String>| v.clone().unwrap_or(strings::VALUE_UNKNOWN.into());
    // The page ends on how the app works rather than on the licence list.
    vec![
        info_row(strings::ABOUT_VERSION, None, env!("CARGO_PKG_VERSION")),
        info_row(strings::ABOUT_NAME, None, &vm.device_name),
        info_row(strings::ABOUT_FIRMWARE, None, &or_unknown(&s.firmware)),
        info_row(strings::ABOUT_CODEC, None, &or_unknown(&s.codec)),
        info_row(
            strings::INFO_ADDRESS,
            None,
            &s.info
                .iter()
                .find(|r| r.label == strings::INFO_ADDRESS)
                .map_or(strings::VALUE_UNKNOWN.into(), |r| r.value.clone()),
        ),
        Button::new()
            .content(strings::OPEN_LOG_FOLDER)
            .automation_name(strings::OPEN_LOG_FOLDER)
            .on_click(cx.message(msg(SettingsMsg::OpenLogFolder)))
            .into(),
        TextBlock::new()
            .text(strings::LICENCES)
            .font_weight(FontWeight::SEMI_BOLD)
            .automation_heading_level(AutomationHeadingLevel::Level3)
            .into(),
        TextBlock::new()
            .text(strings::LICENCE_APP)
            .font_size(12.0)
            .text_wrapping(TextWrapping::Wrap)
            .into(),
        TextBlock::new()
            .text(strings::LICENCES_TEXT)
            .font_size(12.0)
            .text_wrapping(TextWrapping::Wrap)
            .into(),
        TextBlock::new()
            .text(strings::HOW_IT_WORKS)
            .font_weight(FontWeight::SEMI_BOLD)
            .automation_heading_level(AutomationHeadingLevel::Level3)
            .into(),
        text(strings::HOW_IT_WORKS_TEXT),
    ]
}

/// The back arrow and title over a page that replaces the main view.
pub fn page_header(title: &str, cx: &ViewContext<'_, Panel>) -> View {
    Grid::new()
        .columns([GridLength::Auto, GridLength::STAR])
        .column_spacing(4.0)
        .children((
            Button::new()
                .style(ButtonStyle::Subtle)
                .content(FontIcon::new().glyph("\u{E72B}").font_size(16.0))
                .automation_name(strings::BACK)
                .on_click(cx.message(PanelMsg::Back))
                .tooltip(strings::BACK),
            TextBlock::new()
                .text(title)
                .font_size(20.0)
                .font_weight(FontWeight::SEMI_BOLD)
                .vertical_alignment(VerticalAlignment::Center)
                .automation_heading_level(AutomationHeadingLevel::Level1)
                .grid_column(1),
        ))
        .into()
}

pub fn view(
    vm: &PanelVm,
    open_at_sign_in: bool,
    tab: SettingsTab,
    cx: &ViewContext<'_, Panel>,
) -> View {
    let s = &vm.settings;
    let mut page: Vec<View> = vec![StackPanel::new()
        .spacing(4.0)
        .children((page_header(strings::SETTINGS, cx), selector(tab, cx)))
        .into()];
    match tab {
        // Readings kept from before a disconnect are stale, so these cards wait for a connection.
        SettingsTab::Headphones if vm.connected => {
            let controls = headphones(s, cx);
            let readings = on_headphones(s);
            if controls.is_empty() && readings.is_empty() {
                page.push(card(vec![text(strings::NO_HEADPHONE_SETTINGS)]));
            }
            if !controls.is_empty() {
                page.push(card(controls));
            }
            if !readings.is_empty() {
                page.push(section(strings::SECTION_ON_HEADPHONES, readings));
            }
        }
        SettingsTab::Headphones => page.push(card(vec![text(strings::NOT_CONNECTED_NOTE)])),
        SettingsTab::App => page.push(card(app(s, open_at_sign_in, cx))),
        SettingsTab::About => page.push(card(about(vm, cx))),
    }
    StackPanel::new().spacing(16.0).children(page).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xm_core::command::AutoPowerOff;

    fn vm() -> SettingsVm {
        SettingsVm {
            listening_mode: Some(ListeningMode::Standard),
            listening_modes: vec![
                (ListeningMode::Standard, String::new()),
                (ListeningMode::Bgm, String::new()),
                (ListeningMode::Cinema, String::new()),
            ],
            room: Some(BgmRoom::Cafe),
            auto_power_off: vec![
                (AutoPowerOff::WhenTakenOff, String::new()),
                (AutoPowerOff::Never, String::new()),
            ],
            auto_power_off_selected: Some(AutoPowerOff::WhenTakenOff),
            sidetone: Some((1, true)),
            sources: vec![
                SourceVm {
                    mac: "a".into(),
                    name: "Desk".into(),
                    connected: true,
                    playing: true,
                    status: String::new(),
                    can_switch: false,
                },
                SourceVm {
                    mac: "b".into(),
                    name: "Phone".into(),
                    connected: true,
                    playing: false,
                    status: String::new(),
                    can_switch: true,
                },
            ],
            ..SettingsVm::default()
        }
    }

    #[test]
    fn listening_mode_switches_turn_the_old_one_off_first() {
        let mut s = vm();
        let lm = |s: &SettingsVm, i| intents(&SettingsMsg::ListeningMode(Some(i)), s);
        assert_eq!(lm(&s, 0), []);
        assert_eq!(lm(&s, 1), [Intent::SetBgm(true, BgmRoom::Cafe)]);
        s.listening_mode = Some(ListeningMode::Bgm);
        assert_eq!(
            lm(&s, 2),
            [
                Intent::SetBgm(false, BgmRoom::Cafe),
                Intent::SetCinema(true)
            ]
        );
        s.listening_mode = Some(ListeningMode::Cinema);
        assert_eq!(lm(&s, 0), [Intent::SetCinema(false)]);
        assert_eq!(lm(&s, 9), []);
    }

    #[test]
    fn room_changes_only_while_bgm_is_on() {
        let mut s = vm();
        let room = |s: &SettingsVm, i| intents(&SettingsMsg::Room(Some(i)), s);
        assert_eq!(room(&s, 0), []);
        s.listening_mode = Some(ListeningMode::Bgm);
        assert_eq!(room(&s, 0), [Intent::SetBgm(true, BgmRoom::MyRoom)]);
        assert_eq!(room(&s, 2), []);
    }

    #[test]
    fn headphone_controls_send_only_changes() {
        let s = vm();
        let go = |m| intents(&m, &s);
        assert_eq!(go(SettingsMsg::AutoPowerOff(Some(0))), []);
        assert_eq!(
            go(SettingsMsg::AutoPowerOff(Some(1))),
            [Intent::SetAutoPowerOff(AutoPowerOff::Never)]
        );
        assert_eq!(go(SettingsMsg::AutoPowerOff(None)), []);
        assert_eq!(go(SettingsMsg::Sidetone(true)), []);
        assert_eq!(
            go(SettingsMsg::Sidetone(false)),
            [Intent::SetGeneralSetting(1, false)]
        );
        assert_eq!(go(SettingsMsg::PlayHere("a".into())), []);
        assert_eq!(
            go(SettingsMsg::PlayHere("b".into())),
            [Intent::SwitchSource("b".into())]
        );
        assert_eq!(go(SettingsMsg::PlayHere("zz".into())), []);
    }

    #[test]
    fn tabs_round_trip_through_their_labels() {
        for t in TABS {
            assert_eq!(tab_from_text(Some(t.label())), Some(t));
        }
        assert_eq!(tab_from_text(Some("Nope")), None);
        assert_eq!(tab_from_text(None), None);
        assert_eq!(SettingsTab::default(), SettingsTab::Headphones);
    }

    #[test]
    fn app_preferences_save_only_changes() {
        let s = Settings::default();
        assert_eq!(app_change(&SettingsMsg::Colour(Some(0)), &s), None);
        assert_eq!(
            app_change(&SettingsMsg::Colour(Some(2)), &s).map(|n| n.headset_colour),
            Some(Some(HeadsetColour::MidnightBlue))
        );
        assert_eq!(
            app_change(&SettingsMsg::Theme(Some(2)), &s).map(|n| n.theme),
            Some(ThemeChoice::Dark)
        );
        assert_eq!(app_change(&SettingsMsg::BatteryInTooltip(true), &s), None);
        assert_eq!(
            app_change(&SettingsMsg::LowBattery(false), &s).map(|n| n.low_battery_notifications),
            Some(false)
        );
        assert_eq!(app_change(&SettingsMsg::Theme(None), &s), None);
        assert_eq!(app_change(&SettingsMsg::OpenAtSignIn(true), &s), None);
    }
}
