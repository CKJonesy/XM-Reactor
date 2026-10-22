//! Minimal stand-in for XM Reactor's main view: a status line, recovery buttons while
//! disconnected, a row of toggle tiles, a listening-mode row and a grid of preset chips whose
//! callbacks capture the preset they were declared for. Every callback logs the preset it was
//! built with, so a UIA driver that logs which chip it tapped can see whether the two ever differ.
//! "Storm" replays the headset flap from a background thread: Connecting, Disconnected and Ready
//! about a millisecond apart.

use std::io::Write;
use windows_reactor::*;

const PRESETS: [&str; 6] = ["Flat", "Gaming", "Heavy", "Clear", "Hard", "Soft"];
const TILES: [&str; 3] = ["Noise", "Speak", "DSEE"];
const MODES: [&str; 3] = ["Standard", "Background music", "Cinema"];

pub fn log(line: &str) {
    let path = std::env::var("PROBE_LOG").unwrap_or_else(|_| "probe.log".into());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        _ = writeln!(f, "{t} {line}");
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Disconnected,
    Connecting,
    Ready,
}

#[derive(Clone)]
enum Msg {
    Flap,
    Storm,
    Phase(Phase),
    DropFirst,
    Rotate,
    TryAgain,
    Chip(&'static str, Option<bool>),
    Tile(&'static str, Option<bool>),
    Mode(&'static str, Option<bool>),
}

struct Probe {
    phase: Phase,
    drop_first: bool,
    rotate: usize,
    selected: &'static str,
    mode: &'static str,
    tiles_on: [bool; 3],
}

impl Probe {
    fn presets(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = PRESETS.to_vec();
        if self.drop_first {
            v.remove(0);
        }
        let by = self.rotate % v.len();
        v.rotate_left(by);
        v
    }
}

fn chip_row(
    labels: &[&'static str],
    selected: &'static str,
    enabled: bool,
    name: &str,
    cx: &mut ViewContext<Probe>,
    msg: fn(&'static str, Option<bool>) -> Msg,
) -> View {
    let chips: Vec<View> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let label = *label;
            ToggleButton::new()
                .content(TextBlock::new().text(label).font_size(12.0))
                .is_checked(selected == label)
                .is_enabled(enabled)
                .automation_name(label)
                .horizontal_alignment(HorizontalAlignment::Stretch)
                .height(36.0)
                .grid_row((i / 3) as i32)
                .grid_column((i % 3) as i32)
                .on_is_checked_changed(cx.callback(move |v: Option<bool>| msg(label, v)))
                .tooltip(label)
                .into()
        })
        .collect();
    Grid::new()
        .columns([GridLength::STAR; 3])
        .rows(vec![GridLength::Auto; chips.len().div_ceil(3)])
        .column_spacing(8.0)
        .row_spacing(8.0)
        .automation_name(name)
        .children(chips)
        .into()
}

impl Component for Probe {
    type Message = Msg;
    type Input = ();

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        log("create");
        Self {
            phase: Phase::Ready,
            drop_first: false,
            rotate: 0,
            selected: "Flat",
            mode: "Standard",
            tiles_on: [false; 3],
        }
    }

    fn update(&mut self, message: Msg, context: &ComponentContext<Self>) {
        match message {
            Msg::Flap => {
                self.phase = if self.phase == Phase::Ready {
                    Phase::Disconnected
                } else {
                    Phase::Ready
                };
                log(&format!("state phase={:?}", self.phase));
            }
            Msg::Phase(p) => {
                self.phase = p;
                log(&format!("state phase={:?}", self.phase));
            }
            Msg::Storm => {
                log("storm requested");
                // The same handle XM Reactor's engine thread uses to post view models.
                let done = context.completion();
                std::thread::spawn(move || {
                    for p in [
                        Phase::Disconnected,
                        Phase::Connecting,
                        Phase::Disconnected,
                        Phase::Ready,
                    ] {
                        let _ = done.clone().complete(Msg::Phase(p));
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                });
            }
            Msg::DropFirst => {
                self.drop_first = !self.drop_first;
                log(&format!("state drop_first={}", self.drop_first));
            }
            Msg::Rotate => {
                self.rotate += 1;
                log(&format!("state rotate={}", self.rotate));
            }
            Msg::TryAgain => log("try again"),
            Msg::Chip(name, v) => {
                log(&format!("chip-callback {name} {v:?}"));
                if v == Some(true) {
                    self.selected = name;
                }
            }
            Msg::Mode(name, v) => {
                log(&format!("mode-callback {name} {v:?}"));
                if v == Some(true) {
                    self.mode = name;
                }
            }
            Msg::Tile(name, v) => {
                log(&format!("tile-callback {name} {v:?}"));
                if let Some(i) = TILES.iter().position(|t| *t == name) {
                    if let Some(on) = v {
                        self.tiles_on[i] = on;
                    }
                }
            }
        }
    }

    fn view(&self, _input: &(), cx: &mut ViewContext<Self>) -> View {
        let connected = self.phase == Phase::Ready;
        let mut body: Vec<View> = vec![TextBlock::new()
            .text(match self.phase {
                Phase::Disconnected => "Not connected",
                Phase::Connecting => "Connecting…",
                Phase::Ready => "Connected",
            })
            .into()];
        if self.phase == Phase::Disconnected {
            body.push(
                Button::new()
                    .content("Try again")
                    .automation_name("Try again")
                    .on_click(cx.message(Msg::TryAgain))
                    .into(),
            );
            body.push(
                HyperlinkButton::new()
                    .content("Open Bluetooth settings")
                    .automation_name("Open Bluetooth settings")
                    .into(),
            );
        }
        let tiles: Vec<View> = TILES
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let name = *name;
                ToggleButton::new()
                    .content(name)
                    .is_checked(self.tiles_on[i])
                    .is_enabled(connected)
                    .automation_name(name)
                    .grid_column(i as i32)
                    .on_is_checked_changed(cx.callback(move |v: Option<bool>| Msg::Tile(name, v)))
                    .tooltip(name)
                    .into()
            })
            .collect();
        body.push(
            Grid::new()
                .columns([GridLength::STAR; 3])
                .column_spacing(8.0)
                .automation_name("Tiles")
                .children(tiles)
                .into(),
        );
        if connected {
            body.push(chip_row(&MODES, self.mode, true, "Listening mode", cx, Msg::Mode));
            let presets = self.presets();
            body.push(chip_row(&presets, self.selected, true, "Chips", cx, Msg::Chip));
        }
        body.push(
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((
                    Button::new()
                        .content("Flap")
                        .automation_name("Flap")
                        .on_click(cx.message(Msg::Flap)),
                    Button::new()
                        .content("Storm")
                        .automation_name("Storm")
                        .on_click(cx.message(Msg::Storm)),
                    Button::new()
                        .content("DropFirst")
                        .automation_name("DropFirst")
                        .on_click(cx.message(Msg::DropFirst)),
                    Button::new()
                        .content("Rotate")
                        .automation_name("Rotate")
                        .on_click(cx.message(Msg::Rotate)),
                ))
                .into(),
        );
        StackPanel::new()
            .spacing(12.0)
            .margin(Thickness::uniform(12.0))
            .children(body)
            .into()
    }
}

struct ProbeApp;

impl Application for ProbeApp {
    type Input = ();
    type Message = ();

    fn create(_input: &(), context: &ApplicationContext<Self>) -> Self {
        context.show_window("main");
        ProbeApp
    }

    fn update(&mut self, _message: (), _context: &ApplicationContext<Self>) {}

    fn view(&self, _input: &(), _context: &ApplicationViewContext<Self>) -> ApplicationView {
        ApplicationView::new().window::<Probe>("main", ())
    }
}

fn main() {
    log("start");
    App::run_application::<ProbeApp>(()).unwrap();
}
