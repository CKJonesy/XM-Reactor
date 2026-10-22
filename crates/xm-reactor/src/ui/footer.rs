//! Footer: Settings (on the main view) and the more-options menu.

use super::panel::{Panel, PanelMsg};
use crate::strings;
use windows_reactor::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    TurnOff,
    ToggleDemo,
    OpenLog,
    Quit,
}

const MENU: [(&str, MenuAction); 4] = [
    ("turn-off", MenuAction::TurnOff),
    ("demo", MenuAction::ToggleDemo),
    ("open-log", MenuAction::OpenLog),
    ("quit", MenuAction::Quit),
];

fn key(a: MenuAction) -> &'static str {
    MENU.iter().find(|(_, m)| *m == a).map_or("", |(k, _)| k)
}

/// The action for a menu key (menu keys are not `Send`, so the panel's message carries this).
pub fn menu_action(k: &Key) -> Option<MenuAction> {
    let k = k.as_str()?;
    MENU.iter().find(|(key, _)| *key == k).map(|(_, a)| *a)
}

fn icon_button(glyph: &str, name: &str) -> Button {
    Button::new()
        .style(ButtonStyle::Subtle)
        .content(FontIcon::new().glyph(glyph).font_size(16.0))
        .automation_name(name)
}

/// The Settings button; a page has its own back arrow in its header, so none while one is open.
fn page_button(page_open: bool) -> Option<(&'static str, &'static str, PanelMsg)> {
    (!page_open).then_some(("\u{E713}", strings::SETTINGS, PanelMsg::OpenSettings))
}

pub fn view(demo: bool, connected: bool, page_open: bool, cx: &ViewContext<'_, Panel>) -> View {
    let turn_off = if connected {
        MenuItem::item(key(MenuAction::TurnOff), strings::MENU_TURN_OFF)
    } else {
        MenuItem::disabled(key(MenuAction::TurnOff), strings::MENU_TURN_OFF)
    };
    let demo_label = if demo {
        strings::MENU_DEMO_ON
    } else {
        strings::MENU_DEMO_OFF
    };
    let menu = Menu::new(
        [
            turn_off,
            MenuItem::separator("sep-1"),
            MenuItem::item(key(MenuAction::ToggleDemo), demo_label),
            MenuItem::item(key(MenuAction::OpenLog), strings::MENU_OPEN_LOG),
            MenuItem::separator("sep-2"),
            MenuItem::item(key(MenuAction::Quit), strings::MENU_QUIT),
        ],
        cx.callback(|k: Key| PanelMsg::Menu(menu_action(&k))),
    );
    let mut buttons: Vec<View> = Vec::new();
    if let Some((glyph, name, msg)) = page_button(page_open) {
        buttons.push(
            icon_button(glyph, name)
                .grid_column(1)
                .on_click(cx.message(msg))
                .tooltip(name)
                .into(),
        );
    }
    buttons.push(
        icon_button("\u{E712}", strings::MORE_OPTIONS)
            .grid_column(2)
            .menu(menu)
            .tooltip(strings::MORE_OPTIONS)
            .into(),
    );
    Grid::new()
        .columns([GridLength::STAR, GridLength::Auto, GridLength::Auto])
        .column_spacing(4.0)
        .children(buttons)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_offers_settings_only_on_the_main_view() {
        assert!(page_button(true).is_none());
        let (_, name, msg) = page_button(false).unwrap();
        assert_eq!(name, "Settings");
        assert!(matches!(msg, PanelMsg::OpenSettings));
    }

    #[test]
    fn menu_keys_round_trip() {
        for (_, a) in MENU {
            assert_eq!(menu_action(&Key::from(key(a))), Some(a));
        }
        assert_eq!(menu_action(&Key::from("nope")), None);
    }
}
