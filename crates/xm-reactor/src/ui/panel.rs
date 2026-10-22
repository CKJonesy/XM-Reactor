//! The main panel: renders the latest `PanelVm` and turns user actions into engine commands.

use super::eq_curve::{Curve, CurveMsg};
use super::footer::MenuAction;
use super::settings::{self as settings_page, SettingsMsg, SettingsTab};
use super::tray::{self, Tray, TrayAction};
use super::{eq_chips, eq_editor, footer, headset_card, listening, tiles};
use crate::engine::{Engine, EngineHandle, UiCommand};
use crate::platform::{flyout, single_instance, startup, toast, tray_rect};
use crate::strings;
use crate::view_model::{PanelVm, SettingsVm, TileKind};
use crate::AppMsg;
use windows_reactor::*;
use xm_core::command::EqPreset;
use xm_core::log_info;
use xm_core::model::Intent;
use xm_core::settings::{self, Settings, ThemeChoice};

#[derive(Debug, Clone)]
pub enum PanelMsg {
    Vm(Box<PanelVm>),
    Tile(TileKind),
    Ambient(f64),
    FocusOnVoice(bool),
    /// The render's rasterisation scale (1.0 = 96 DPI).
    Scale(f64),
    Preset(EqPreset),
    Curve(CurveMsg),
    ClearBass(f64),
    ResetFlat,
    RenameOpen,
    RenameText(String),
    /// True when the user chose Rename.
    RenameClosed(bool),
    /// Start a fresh connection attempt now.
    TryAgain,
    /// The recovery card's link to the Windows Bluetooth settings page.
    OpenBluetoothSettings,
    /// Esc: back to the main view from a page, otherwise hide the flyout.
    Escape,
    OpenSettings,
    /// The settings page's selector bar changed; None when it reports no selection.
    SettingsTab(Option<SettingsTab>),
    /// A page's back arrow.
    Back,
    Settings(SettingsMsg),
    Menu(Option<MenuAction>),
    PowerOffConfirmed,
    DialogDismissed,
    Tray(Option<TrayAction>),
    /// Windows changed a setting, perhaps the taskbar theme the tray icon follows.
    SettingChanged,
    /// Measured heights in DIP of the scrolling content and of the footer.
    ContentHeight(f64),
    FooterHeight(f64),
    /// `single_instance::OPEN` or `QUIT` from another process or the toast.
    Request(usize),
}

pub struct Panel {
    engine: EngineHandle,
    /// The application, which owns the tray icon and quits.
    app: Callback<AppMsg>,
    tray_menu: Callback<Key>,
    /// The tray last handed to the application.
    tray: Option<Tray>,
    /// Re-read on Windows' settings-change broadcast.
    light_taskbar: bool,
    /// None until the engine's first publish.
    vm: Option<PanelVm>,
    demo: bool,
    /// The saved settings; app preference changes edit this copy and send it to the engine.
    settings: Settings,
    /// The page shown in place of the main view, at the same height.
    page: Page,
    /// Which of the settings page's views is open; back to Headphones each time the page opens.
    settings_tab: SettingsTab,
    /// The equaliser curve's selected band and drag.
    curve: Curve,
    /// Whether "Open at sign-in" is on, read from the registry.
    open_at_sign_in: bool,
    confirm_power_off: bool,
    /// The custom preset being renamed and the name typed so far.
    rename: Option<(EqPreset, String)>,
    render: ElementRef<Image>,
    scale: f64,
    /// Measured for the flyout's height (see `fit`).
    content: ElementRef<Grid>,
    footer: ElementRef<Grid>,
    content_height: f64,
    footer_height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Main,
    Settings,
}

pub fn window_theme(t: ThemeChoice) -> WindowTheme {
    match t {
        ThemeChoice::FollowWindows => WindowTheme::System,
        ThemeChoice::Light => WindowTheme::Light,
        ThemeChoice::Dark => WindowTheme::Dark,
    }
}

impl Panel {
    /// Sizes the flyout to the measured content and footer, once both are known.
    fn fit(&self) {
        if self.content_height > 0.0 && self.footer_height > 0.0 {
            crate::set_height(self.content_height + self.footer_height);
        }
    }

    fn send(&self, intent: Option<Intent>) {
        if let Some(i) = intent {
            xm_core::log_debug!("ui: intent {i:?}");
            self.engine.send(UiCommand::Intent(i));
        }
    }

    /// Sends a band change and shows it at once, so the next change builds on it rather than on
    /// a publish that has not caught up.
    fn send_eq(&mut self, intent: Option<Intent>) {
        if let (Some(Intent::SetEqBands(cb, bands)), Some(vm)) = (&intent, self.vm.as_mut()) {
            vm.settings.clear_bass = *cb;
            vm.settings.bands = bands.clone();
            self.curve.hold(bands);
        }
        self.send(intent);
    }

    fn on_menu(&mut self, action: MenuAction, context: &ComponentContext<Self>) {
        match action {
            MenuAction::TurnOff => {
                self.confirm_power_off = self.vm.as_ref().is_some_and(|vm| vm.connected);
            }
            MenuAction::ToggleDemo => {
                self.demo = !self.demo;
                self.engine.send(UiCommand::SetDemo(self.demo));
            }
            MenuAction::OpenLog => {
                open_in_explorer(&settings::app_dir().join("logs").join("app.log"))
            }
            MenuAction::Quit => {
                self.engine.send(UiCommand::Quit);
                // The tray icon keeps the application alive, so closing the window is not enough.
                self.app.call(AppMsg::Quit);
                let _ = context.close_window();
            }
        }
    }

    /// Hands the application a new tray when the icon, tooltip or menu changed.
    fn publish_tray(&mut self) {
        let t = tray::tray(
            self.vm.as_ref(),
            self.open_at_sign_in,
            self.light_taskbar,
            &self.tray_menu,
        );
        if t == self.tray {
            return;
        }
        if let Some(t) = &t {
            let icon = t.icon.file_name().unwrap_or_default().to_string_lossy();
            log_info!("tray: {icon} · \"{}\" · {:?}", t.tooltip, t.menu);
            self.app.call(AppMsg::Tray(t.clone()));
        }
        self.tray = t;
    }

    /// One tap on "Pause when taken off" sets the headphones' auto-pause and the app's own pause
    /// and resume of Windows media together (Ruling 23).
    fn on_tile(&mut self, kind: TileKind) {
        let Some((intent, save)) = self.vm.as_ref().and_then(|vm| {
            let tile = vm.tiles.iter().find(|t| t.kind == kind)?;
            tiles::tile_tap(tile, vm.connected, &self.settings)
        }) else {
            return;
        };
        if let Intent::SetAutoPause(on) = intent {
            log_info!("pause when taken off: headphones and app pause and resume {on}");
        }
        if let Some(s) = save {
            self.settings = s.clone();
            self.engine.send(UiCommand::SaveSettings(s));
        }
        self.send(Some(intent));
    }

    fn on_settings(&mut self, m: SettingsMsg) {
        match m {
            SettingsMsg::OpenAtSignIn(on) if on != self.open_at_sign_in => {
                if let Err(e) = startup::set(startup::RUN_KEY, on) {
                    log_info!("could not change open at sign-in: {e}");
                }
                self.open_at_sign_in = startup::is_enabled(startup::RUN_KEY);
            }
            SettingsMsg::OpenLogFolder => open_in_explorer(&settings::app_dir().join("logs")),
            m => {
                if let Some(s) = settings_page::app_change(&m, &self.settings) {
                    self.settings = s.clone();
                    self.engine.send(UiCommand::SaveSettings(s));
                }
                if let Some(vm) = self.vm.as_ref().filter(|vm| vm.connected) {
                    for i in settings_page::intents(&m, &vm.settings) {
                        self.engine.send(UiCommand::Intent(i));
                    }
                }
            }
        }
    }
}

/// Hands a folder or a URI such as ms-settings:bluetooth to the shell, which opens and raises it.
fn open_in_explorer(path: &std::path::Path) {
    if let Err(e) = std::process::Command::new("explorer.exe").arg(path).spawn() {
        log_info!("could not open {}: {e}", path.display());
    }
}

impl Component for Panel {
    type Input = Callback<AppMsg>;
    type Message = PanelMsg;

    fn create(app: &Callback<AppMsg>, context: &ComponentContext<Self>) -> Self {
        let args = crate::args::parse(std::env::args().skip(1));
        let saved = Settings::load(&settings::default_path());
        // The engine thread only enqueues messages; the UI thread applies them.
        let done = context.completion();
        let engine = Engine::spawn(&args, saved.clone(), move |vm| {
            let _ = done.clone().complete(PanelMsg::Vm(Box::new(vm)));
        });
        let changed = context.sender();
        flyout::on_setting_change(move || {
            _ = changed.send(PanelMsg::SettingChanged);
        });
        let requests = context.sender();
        flyout::on_request(move |r| {
            _ = requests.send(PanelMsg::Request(r));
        });
        let mut panel = Self {
            engine,
            app: app.clone(),
            tray_menu: context.callback(|k: Key| PanelMsg::Tray(tray::menu_action(&k))),
            tray: None,
            light_taskbar: tray_rect::light_taskbar(),
            vm: None,
            demo: args.demo,
            settings: saved,
            page: Page::Main,
            curve: Curve::default(),
            open_at_sign_in: startup::is_enabled(startup::RUN_KEY),
            settings_tab: SettingsTab::default(),
            confirm_power_off: false,
            rename: None,
            render: ElementRef::new(),
            content: ElementRef::new(),
            footer: ElementRef::new(),
            content_height: 0.0,
            footer_height: 0.0,
            scale: 1.0,
        };
        panel.publish_tray();
        panel
    }

    fn update(&mut self, message: PanelMsg, context: &ComponentContext<Self>) {
        // Tray menu items do what the matching tile, setting or menu item does.
        let message = match message {
            PanelMsg::Tray(Some(TrayAction::Mode(kind))) => PanelMsg::Tile(kind),
            PanelMsg::Tray(Some(TrayAction::ToggleOpenAtSignIn)) => {
                PanelMsg::Settings(SettingsMsg::OpenAtSignIn(!self.open_at_sign_in))
            }
            PanelMsg::Tray(Some(TrayAction::Quit)) => PanelMsg::Menu(Some(MenuAction::Quit)),
            PanelMsg::Request(single_instance::QUIT) => PanelMsg::Menu(Some(MenuAction::Quit)),
            m => m,
        };
        let vm = self.vm.as_ref();
        match message {
            PanelMsg::Vm(mut vm) => {
                if let Some(level) = vm.low_battery_toast {
                    toast::show_low_battery(&vm.device_name, level);
                }
                if !vm.connected {
                    self.confirm_power_off = false;
                }
                let preset = |v: &PanelVm| v.eq.as_ref().map(|e| e.selected);
                if !(vm.connected && vm.settings.show_bands) {
                    self.curve = Curve::default();
                } else if self.vm.as_ref().map(preset) != Some(preset(&vm)) {
                    self.curve.forget();
                }
                self.curve.merge(&mut vm.settings);
                self.vm = Some(*vm);
            }
            PanelMsg::Tile(kind) => self.on_tile(kind),
            PanelMsg::Ambient(value) => self.send(vm.and_then(|vm| {
                vm.ambient
                    .as_ref()
                    .filter(|_| vm.connected)
                    .and_then(|a| tiles::ambient_intent(value, a))
            })),
            PanelMsg::FocusOnVoice(on) => self.send(vm.and_then(|vm| {
                vm.ambient
                    .as_ref()
                    .filter(|_| vm.connected)
                    .and_then(|a| tiles::focus_intent(on, a))
            })),
            PanelMsg::Preset(p) => self.send(vm.and_then(|vm| {
                vm.eq
                    .as_ref()
                    .filter(|_| vm.connected)
                    .and_then(|eq| eq_chips::preset_intent(eq, p))
            })),
            // Ends and focus changes go through even when disconnected, so no gesture outlives it.
            PanelMsg::Curve(m) => {
                let none = SettingsVm::default();
                let intent = self
                    .curve
                    .update(m, vm.map_or(&none, |vm| &vm.settings))
                    .filter(|_| vm.is_some_and(|vm| vm.connected));
                self.send_eq(intent);
            }
            PanelMsg::ClearBass(v) => self.send_eq(
                vm.filter(|vm| vm.connected)
                    .and_then(|vm| eq_editor::clear_bass_intent(&vm.settings, v)),
            ),
            PanelMsg::ResetFlat => self.send_eq(
                vm.filter(|vm| vm.connected)
                    .and_then(|vm| eq_editor::reset_intent(&vm.settings)),
            ),
            PanelMsg::Escape | PanelMsg::Back if self.page != Page::Main => self.page = Page::Main,
            PanelMsg::Escape => flyout::hide(flyout::current()),
            PanelMsg::Back => {}
            PanelMsg::Scale(s) => self.scale = s,
            PanelMsg::ContentHeight(h) => {
                self.content_height = h;
                self.fit();
            }
            PanelMsg::FooterHeight(h) => {
                self.footer_height = h;
                self.fit();
            }
            // While a page is open the footer button is Back (see `footer::view`).
            PanelMsg::OpenSettings => {
                self.page = Page::Settings;
                self.settings_tab = SettingsTab::default();
            }
            PanelMsg::SettingsTab(Some(t)) => self.settings_tab = t,
            PanelMsg::SettingsTab(None) => {}
            PanelMsg::TryAgain => self.engine.send(UiCommand::Reconnect),
            PanelMsg::OpenBluetoothSettings => {
                log_info!("opening {}", headset_card::BLUETOOTH_SETTINGS);
                open_in_explorer(std::path::Path::new(headset_card::BLUETOOTH_SETTINGS));
            }
            PanelMsg::Settings(m) => self.on_settings(m),
            PanelMsg::Menu(Some(action)) => self.on_menu(action, context),
            PanelMsg::Menu(None) => {}
            PanelMsg::PowerOffConfirmed => {
                self.confirm_power_off = false;
                if self.vm.as_ref().is_some_and(|vm| vm.connected) {
                    self.engine.send(UiCommand::Intent(Intent::PowerOff));
                }
            }
            PanelMsg::DialogDismissed => self.confirm_power_off = false,
            PanelMsg::SettingChanged => self.light_taskbar = tray_rect::light_taskbar(),
            // The tray is republished below for every message.
            PanelMsg::Tray(_) => {}
            PanelMsg::Request(_) => self.app.call(AppMsg::Open),
            PanelMsg::RenameOpen => {
                self.rename = vm
                    .filter(|vm| vm.connected && vm.settings.custom_preset)
                    .and_then(|vm| vm.eq.as_ref())
                    .and_then(|eq| eq.presets.iter().find(|(p, _)| *p == eq.selected))
                    .map(|(p, label)| (*p, label.clone()));
            }
            PanelMsg::RenameText(t) => {
                if let Some((_, draft)) = &mut self.rename {
                    *draft = t.chars().take(eq_editor::NAME_MAX).collect();
                }
            }
            PanelMsg::RenameClosed(confirmed) => {
                let renamed = self
                    .rename
                    .take()
                    .filter(|_| confirmed)
                    .and_then(|(p, draft)| eq_editor::rename(&self.settings, p, &draft));
                if let Some(s) = renamed {
                    self.settings = s.clone();
                    self.engine.send(UiCommand::SaveSettings(s));
                }
            }
        }
        self.publish_tray();
    }

    fn view(&self, _: &Callback<AppMsg>, context: &mut ViewContext<'_, Self>) -> View {
        let theme = self
            .vm
            .as_ref()
            .map_or(self.settings.theme, |vm| vm.settings.app.theme);
        context.window_title(strings::WINDOW_TITLE);
        let (width, height) = crate::flyout_size();
        context.window_visuals(
            WindowVisuals::new()
                // Reactor shows the new window before `make_flyout` restyles it, which put a
                // captioned window on screen for about 100 ms at start-up; it now opens off screen.
                .initial_position(ScreenPoint::new(-32000, -32000))
                .client_size(width, height)
                .theme(window_theme(theme))
                .backdrop(WindowBackdrop::Mica),
        );
        // Follows the render's scale live, so a move to another monitor picks the right cut-out.
        let render = self.render.clone();
        let sender = context.sender();
        context.use_effect_guard("render-scale", (), move || {
            render.observe_rasterization_scale(move |s| {
                _ = sender.send(PanelMsg::Scale(s));
            })
        });
        // The flyout fits its content (see `fit`).
        for (key, element, msg) in [
            (
                "content-height",
                self.content.clone(),
                PanelMsg::ContentHeight as fn(f64) -> PanelMsg,
            ),
            ("footer-height", self.footer.clone(), PanelMsg::FooterHeight),
        ] {
            let sender = context.sender();
            context.use_effect_guard(key, (), move || {
                element.observe_composition_host(move |e| {
                    let (CompositionHostEvent::Ready { height, .. }
                    | CompositionHostEvent::Metrics { height, .. }) = e;
                    _ = sender.send(msg(height));
                })
            });
        }
        // Keyboard focus sits in the XAML content, so Esc arrives here rather than at the window.
        let keys = context.sender();
        let on_key = RoutedCallback::new(move |k: KeyEventInfo| {
            let esc = k.key == VirtualKey::ESCAPE;
            if esc {
                _ = keys.send(PanelMsg::Escape);
            }
            esc
        });
        let cx = &*context;
        let mut body: Vec<View> = Vec::new();
        let connected = self.vm.as_ref().is_some_and(|vm| vm.connected);
        let settings_open = self.page == Page::Settings;
        if let Some(vm) = self.vm.as_ref().filter(|_| settings_open) {
            body.push(settings_page::view(
                vm,
                self.open_at_sign_in,
                self.settings_tab,
                cx,
            ));
        } else if let Some(vm) = &self.vm {
            body.push(headset_card::view(vm, self.scale, &self.render, cx));
            body.push(tiles::view(&vm.tiles, cx));
            // The level and Focus on voice only apply in Ambient mode, so they show only then.
            if let Some(a) = vm.ambient.as_ref().filter(|a| a.enabled) {
                body.push(tiles::ambient_view(a, vm.connected, cx));
            }
            if !vm.connected {
                body.push(
                    TextBlock::new()
                        .text(strings::DISCONNECTED_HINT)
                        .font_size(12.0)
                        .text_wrapping(TextWrapping::Wrap)
                        .into(),
                );
            }
            if vm.connected && !vm.settings.listening_modes.is_empty() {
                body.push(listening::view(&vm.settings, cx));
            }
            if let Some(eq) = &vm.eq {
                body.push(eq_chips::view(eq, cx));
                // The flyout is tall while this shows (Ruling 29), so it sits in view.
                if vm.connected && vm.settings.show_bands {
                    body.push(eq_editor::view(&vm.settings, &self.curve, cx));
                }
            }
        }
        let page = Grid::new()
            .rows([GridLength::STAR, GridLength::Auto])
            .children((
                ScrollViewer::new().content(
                    // The inner Grid is measured; it spans the stack and its margins. It sits in
                    // an Auto row, so it keeps its natural height while the scroller stretches
                    // the outer Grid to the viewport.
                    Grid::new()
                        .rows([GridLength::Auto, GridLength::STAR])
                        .children((Grid::new().element_ref(&self.content).children((
                            StackPanel::new()
                                .spacing(12.0)
                                .margin(Thickness::new(12.0, 12.0, 12.0, 12.0))
                                .children(body),
                        )),)),
                ),
                Grid::new()
                    .element_ref(&self.footer)
                    .grid_row(1)
                    .children((Border::new()
                        .padding(Thickness::new(12.0, 0.0, 8.0, 8.0))
                        .content(footer::view(
                            self.demo,
                            connected,
                            self.page != Page::Main,
                            cx,
                        )),)),
            ))
            .content_dialog(
                ContentDialog::new()
                    .title(strings::TURN_OFF_TITLE)
                    .content(
                        TextBlock::new()
                            .text(strings::TURN_OFF_BODY)
                            .text_wrapping(TextWrapping::Wrap),
                    )
                    .primary_button_text(strings::TURN_OFF_CONFIRM)
                    .close_button_text(strings::TURN_OFF_CANCEL)
                    .default_button(ContentDialogButton::Close)
                    .is_open(self.confirm_power_off)
                    .on_closed(cx.callback(|r: ContentDialogResult| {
                        if matches!(r, ContentDialogResult::Primary) {
                            PanelMsg::PowerOffConfirmed
                        } else {
                            PanelMsg::DialogDismissed
                        }
                    })),
            );
        let draft = self.rename.as_ref().map_or("", |(_, d)| d.as_str());
        // One dialog per element, so the rename dialog sits on the outer border.
        Border::new()
            .on_preview_key_down(on_key)
            .content(page)
            .content_dialog(
                ContentDialog::new()
                    .title(strings::RENAME_PRESET)
                    .content(
                        StackPanel::new().spacing(8.0).children((
                            TextBox::new()
                                .text(draft)
                                .automation_name(strings::PRESET_NAME)
                                .on_text_changed(cx.callback(|t: std::rc::Rc<str>| {
                                    PanelMsg::RenameText(t.to_string())
                                })),
                            TextBlock::new()
                                .text(strings::RENAME_HINT)
                                .font_size(12.0)
                                .text_wrapping(TextWrapping::Wrap),
                        )),
                    )
                    .primary_button_text(strings::RENAME)
                    .close_button_text(strings::RENAME_CANCEL)
                    .default_button(ContentDialogButton::Primary)
                    .is_open(self.rename.is_some())
                    .on_closed(cx.callback(|r: ContentDialogResult| {
                        PanelMsg::RenameClosed(matches!(r, ContentDialogResult::Primary))
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_choice_maps_to_window_theme() {
        assert_eq!(
            window_theme(ThemeChoice::FollowWindows),
            WindowTheme::System
        );
        assert_eq!(window_theme(ThemeChoice::Light), WindowTheme::Light);
        assert_eq!(window_theme(ThemeChoice::Dark), WindowTheme::Dark);
    }
}
