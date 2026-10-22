#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod engine;
mod platform;
mod strings;
mod ui;
#[allow(dead_code)] // the settings page and tray read the rest
mod view_model;

use platform::{flyout, placement, single_instance, toast, tray_rect};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use windows::core::HSTRING;
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows_reactor::*;
use xm_core::log_info;

/// The flyout's width, and its height until the panel has measured its content.
const FLYOUT_SIZE_DIP: (f64, f64) = (360.0, 730.0);

/// The height the panel's content wants, in whole DIP; changed only by `set_height`.
static HEIGHT: AtomicU32 = AtomicU32::new(FLYOUT_SIZE_DIP.1 as u32);

pub enum AppMsg {
    /// The panel's latest tray icon, tooltip and menu.
    Tray(ui::tray::Tray),
    /// A tray icon click or keyboard selection at this screen point.
    Activate(ScreenPoint),
    /// Another launch or the low-battery toast asked for the flyout.
    Open,
    Quit,
}

struct XmReactor {
    tray: Option<ui::tray::Tray>,
    /// Kept so the window's input stays equal between views.
    forward: Callback<AppMsg>,
}

impl Application for XmReactor {
    /// Whether to open the flyout at start-up (`--open`).
    type Input = bool;
    type Message = AppMsg;

    fn create(_: &bool, context: &ApplicationContext<Self>) -> Self {
        // The window must exist for the tray to toggle it; it hides itself unless `--open`.
        context.show_window("main");
        Self {
            tray: None,
            forward: context.forward(),
        }
    }

    fn update(&mut self, message: AppMsg, context: &ApplicationContext<Self>) {
        match message {
            AppMsg::Tray(t) => self.tray = Some(t),
            AppMsg::Activate(p) => toggle_flyout(p),
            AppMsg::Open => {
                let hwnd = flyout::current();
                if hwnd != 0 {
                    flyout::clear_reopen_guard();
                    flyout::show_at(hwnd, flyout_rect(None));
                }
            }
            AppMsg::Quit => context.exit(),
        }
    }

    fn view(&self, open: &bool, context: &ApplicationViewContext<Self>) -> ApplicationView {
        let input = MainInput {
            open: *open,
            app: self.forward.clone(),
        };
        let view = ApplicationView::new().window::<MainWindow>("main", input);
        match &self.tray {
            Some(t) => view.notify_icon(
                "tray",
                NotifyIcon::new(&t.icon)
                    .tooltip(&t.tooltip)
                    .on_activate(context.callback(AppMsg::Activate))
                    .menu(t.menu.clone()),
            ),
            None => view,
        }
    }
}

/// Hides the open flyout, or opens it beside the tray icon at `p`.
fn toggle_flyout(p: ScreenPoint) {
    let hwnd = flyout::current();
    if hwnd == 0 {
        return;
    }
    if flyout::is_open(hwnd) {
        flyout::hide(hwnd);
        return;
    }
    // The click point stands in for the icon's rect, which reactor keeps private.
    let icon = placement::Rect {
        left: p.x,
        top: p.y,
        right: p.x + 1,
        bottom: p.y + 1,
    };
    flyout::show_at(hwnd, flyout_rect(Some(icon)));
}

/// The taskbar's screen, or the primary work area when the taskbar cannot be queried.
fn screen() -> tray_rect::TaskbarScreen {
    tray_rect::taskbar_screen().unwrap_or_else(tray_rect::primary_screen)
}

/// The flyout's size in DIP: the wanted height, capped to fit the work area.
fn flyout_size_on(s: &tray_rect::TaskbarScreen, height_dip: f64) -> (f64, f64) {
    let h = placement::fit_height(height_dip, s.work_area, s.dpi_scale);
    (FLYOUT_SIZE_DIP.0, h)
}

fn wanted_height() -> f64 {
    f64::from(HEIGHT.load(Relaxed))
}

pub fn flyout_size() -> (f64, f64) {
    flyout_size_on(&screen(), wanted_height())
}

/// Sizes the flyout to its content; an open flyout keeps its taskbar edge and grows or shrinks
/// away from it. Does nothing unless the whole-DIP height changes.
pub fn set_height(dip: f64) {
    let wanted = dip.ceil().max(0.0) as u32;
    if HEIGHT.swap(wanted, Relaxed) == wanted {
        return;
    }
    let s = screen();
    let h = (flyout_size_on(&s, f64::from(wanted)).1 * s.dpi_scale).round() as i32;
    let keep_top = s.edge == placement::TaskbarEdge::Top;
    flyout::resize(flyout::current(), h, s.work_area, keep_top);
}

/// Flyout placement beside the tray icon (`None`: the tray corner) on the taskbar's monitor,
/// or the primary work area's bottom-right corner when the taskbar cannot be queried.
fn flyout_rect(icon: Option<placement::Rect>) -> placement::Rect {
    let s = tray_rect::taskbar_screen().unwrap_or_else(|| {
        log_info!("taskbar position unavailable; placing the flyout on the primary work area");
        tray_rect::primary_screen()
    });
    placement::place(
        s.work_area,
        s.monitor,
        s.edge,
        icon,
        flyout_size_on(&s, wanted_height()),
        s.dpi_scale,
    )
}

#[derive(Clone, PartialEq)]
struct MainInput {
    open: bool,
    app: Callback<AppMsg>,
}

struct MainWindow {
    open: bool,
}

enum MainMsg {
    Hwnd(isize),
}

impl Component for MainWindow {
    type Input = MainInput;
    type Message = MainMsg;

    fn create(input: &MainInput, context: &ComponentContext<Self>) -> Self {
        let _ = context.run_window(|w| MainMsg::Hwnd(w.as_raw() as isize));
        Self { open: input.open }
    }

    fn update(&mut self, message: MainMsg, _: &ComponentContext<Self>) {
        let MainMsg::Hwnd(hwnd) = message;
        flyout::make_flyout(hwnd);
        // With no tray icon the flyout is the only way to reach Quit, so it opens.
        if self.open || ui::tray::icon_dir().is_none() {
            flyout::show_at(hwnd, flyout_rect(None));
        }
    }

    fn view(&self, input: &MainInput, _: &mut ViewContext<Self>) -> View {
        // The panel publishes the window's title and visuals, since it owns the theme setting,
        // and handles Esc, since Esc on the settings page goes back rather than hiding.
        // A window's root must be an element, so the panel sits in a plain Border.
        Border::new()
            .content(component::<ui::panel::Panel>("panel", input.app.clone()))
            .into()
    }
}

fn main() {
    let args = args::parse(std::env::args().skip(1));
    xm_core::log::init_log(args.verbose);
    if args.uninstall_cleanup {
        platform::cleanup::run(args.quiet);
        return;
    }
    if !single_instance::claim() {
        log_info!("XM Reactor is already running; asked it to open the flyout");
        return;
    }
    // The toasts' AUMID, so Windows ties them to this process.
    // SAFETY: the HSTRING outlives the call.
    if let Err(e) = unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(toast::AUMID)) }
    {
        log_info!("could not set the app's AUMID: {e}");
    }
    if !platform::runtime::runtime_available() {
        log_info!("Windows App Runtime 2.x not found");
        platform::runtime::show_runtime_missing();
        return;
    }
    log_info!("XM Reactor started");
    if let Err(e) = App::run_application::<XmReactor>(args.open) {
        log_info!("application stopped with an error: {e:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flyout_height_is_capped_to_the_work_area() {
        let screen = |bottom| tray_rect::TaskbarScreen {
            edge: placement::TaskbarEdge::Bottom,
            work_area: placement::Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom,
            },
            monitor: placement::Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            dpi_scale: 1.0,
        };
        assert_eq!(flyout_size_on(&screen(1032), 730.0), FLYOUT_SIZE_DIP);
        assert_eq!(flyout_size_on(&screen(1032), 540.0), (360.0, 540.0));
        // Capped to the work area less the 12 DIP gaps.
        assert_eq!(flyout_size_on(&screen(700), 730.0), (360.0, 676.0));
        assert_eq!(flyout_size_on(&screen(700), 917.0), (360.0, 676.0));
    }

    #[test]
    fn runtime_missing_message() {
        assert!(!crate::strings::RUNTIME_MISSING.is_empty());
        assert!(crate::strings::RUNTIME_MISSING.contains("Windows App Runtime"));
    }
}
