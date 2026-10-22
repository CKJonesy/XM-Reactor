//! The engine thread: owns the session and the model, and publishes display-ready view models.
//! Nothing here touches UI objects; `publish` is the only way out.

use crate::args::Args;
use crate::view_model::{self, PanelVm};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;
use xm_core::bluetooth::RfcommConnector;
use xm_core::clock::SystemClock;
use xm_core::demo::DemoConnector;
use xm_core::low_battery::LowBatteryMonitor;
use xm_core::media::{GsmtcMediaControl, MediaControl, NullMediaControl};
use xm_core::model::{BehaviourPrefs, Effect, Intent, Model};
use xm_core::runner::{self, SessionHandle};
use xm_core::session::SessionState;
use xm_core::settings::{self, Settings};
use xm_core::transport::Connector;
use xm_core::{log_debug, log_info};

const SLICE: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq)]
pub enum UiCommand {
    Intent(Intent),
    SetDemo(bool),
    SaveSettings(Settings),
    /// Start a fresh session now rather than waiting out the backoff ("Try again").
    Reconnect,
    Quit,
}

pub struct EngineHandle {
    tx: Sender<UiCommand>,
}

impl EngineHandle {
    pub fn send(&self, c: UiCommand) {
        // A send failure means the engine has already quit.
        let _ = self.tx.send(c);
    }
}

pub struct Engine;

impl Engine {
    pub fn spawn(
        args: &Args,
        settings: Settings,
        publish: impl Fn(PanelVm) + Send + 'static,
    ) -> EngineHandle {
        let (tx, rx) = mpsc::channel();
        let demo_battery = args.demo_battery;
        let demo_name = args.demo_name.clone();
        let demo = args.demo;
        std::thread::spawn(move || {
            Worker::new(demo, demo_battery, demo_name, settings, publish).run(rx);
        });
        EngineHandle { tx }
    }
}

struct Worker<P: Fn(PanelVm)> {
    demo: bool,
    demo_battery: Option<u8>,
    demo_name: Option<String>,
    settings: Settings,
    publish: P,
    model: Model,
    session: SessionHandle,
    events_open: bool,
    monitor: LowBatteryMonitor,
    /// Set when the app turns the headphones off; cleared when a link comes back.
    turned_off: bool,
    /// Whether the last published state had a link (connecting or ready).
    linked: bool,
}

/// Power-off leaves the link up for a moment, so only a link that comes back (connecting or ready
/// after being down) clears the turned-off flag.
fn still_turned_off(turned_off: bool, was_linked: bool, linked: bool) -> bool {
    turned_off && !(linked && !was_linked)
}

fn prefs(s: &Settings) -> BehaviourPrefs {
    BehaviourPrefs {
        pause_on_removal: s.pause_on_removal,
        resume_on_wear: s.resume_on_wear,
    }
}

impl<P: Fn(PanelVm)> Worker<P> {
    fn new(
        demo: bool,
        demo_battery: Option<u8>,
        demo_name: Option<String>,
        settings: Settings,
        publish: P,
    ) -> Self {
        let (model, session) = Self::start(demo, demo_battery, demo_name.as_deref(), &settings);
        Self {
            demo,
            demo_battery,
            demo_name,
            settings,
            publish,
            model,
            session,
            events_open: true,
            monitor: LowBatteryMonitor::default(),
            turned_off: false,
            linked: false,
        }
    }

    /// A fresh model and session; the demo never touches the owner's media.
    fn start(
        demo: bool,
        battery: Option<u8>,
        name: Option<&str>,
        settings: &Settings,
    ) -> (Model, SessionHandle) {
        let (connector, media): (Box<dyn Connector>, Box<dyn MediaControl>) = if demo {
            let mut c = DemoConnector::new();
            if let Some(n) = battery {
                c = c.with_battery(n);
            }
            if let Some(n) = name {
                c = c.with_name(n);
            }
            (Box::new(c), Box::new(NullMediaControl))
        } else {
            (Box::new(RfcommConnector), Box::new(GsmtcMediaControl))
        };
        let clock = Arc::new(SystemClock);
        let mut model = Model::new(media, prefs(settings), clock.clone());
        model.set_eq_curves(settings.eq_curves.clone());
        let session = runner::start(connector, clock, settings.preferred_address.clone());
        (model, session)
    }

    fn run(mut self, rx: mpsc::Receiver<UiCommand>) {
        self.publish();
        loop {
            let mut changed = false;
            if self.events_open {
                match self.session.events.recv_timeout(SLICE) {
                    Ok(e) => {
                        let fx = self.model.on_event(e);
                        changed |= self.run_effects(fx);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => self.events_open = false,
                }
            } else {
                std::thread::sleep(SLICE);
            }
            loop {
                match rx.try_recv() {
                    Ok(UiCommand::Quit) | Err(mpsc::TryRecvError::Disconnected) => {
                        self.quit();
                        return;
                    }
                    Ok(c) => changed |= self.on_command(c),
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
            let fx = self.model.on_tick();
            changed |= self.run_effects(fx);
            if changed {
                self.save_curves();
                self.publish();
            }
        }
    }

    /// Replaces the model and session with fresh ones; the old session stops on its own thread.
    fn restart(&mut self) {
        let (model, session) = Self::start(
            self.demo,
            self.demo_battery,
            self.demo_name.as_deref(),
            &self.settings,
        );
        self.model = model;
        let old = std::mem::replace(&mut self.session, session);
        self.events_open = true;
        self.monitor = LowBatteryMonitor::default();
        self.turned_off = false;
        std::thread::spawn(move || old.stop());
    }

    fn on_command(&mut self, c: UiCommand) -> bool {
        match c {
            UiCommand::Intent(i) => {
                let power_off = i == Intent::PowerOff;
                let fx = self.model.apply(i);
                // Only a power-off that actually goes out to the headphones counts.
                self.turned_off |= power_off && fx.iter().any(|e| matches!(e, Effect::Send(_)));
                self.run_effects(fx) || power_off
            }
            UiCommand::SetDemo(on) if on != self.demo => {
                log_info!("demo mode: {on}");
                self.demo = on;
                self.restart();
                true
            }
            UiCommand::SetDemo(_) => false,
            UiCommand::Reconnect => {
                log_info!("reconnect requested");
                self.restart();
                true
            }
            UiCommand::SaveSettings(s) => {
                // The engine owns the remembered curves; the panel's copy may be older.
                let s = Settings {
                    eq_curves: self.settings.eq_curves.clone(),
                    ..s
                };
                if let Err(e) = s.save(&settings::default_path()) {
                    log_info!("could not save settings: {e}");
                }
                self.model.set_prefs(prefs(&s));
                self.settings = s;
                true
            }
            // `run` handles Quit before dispatching here.
            UiCommand::Quit => false,
        }
    }

    /// Sends what the model asks for; true when the state changed.
    fn run_effects(&mut self, fx: Vec<Effect>) -> bool {
        let mut changed = false;
        for e in fx {
            match e {
                Effect::Send(c) => {
                    let name = c.name;
                    let id = self.session.send(c);
                    self.model.on_sent(id, name);
                }
                Effect::StateChanged => changed = true,
                // The model already paused or resumed playback itself.
                Effect::Media(a) => log_debug!("media: {a:?}"),
            }
        }
        changed
    }

    /// Saves the remembered preset curves when a report changed them. Never in the demo, whose
    /// made-up curves would replace the real headset's under the same name.
    fn save_curves(&mut self) {
        if self.demo || self.model.eq_curves() == &self.settings.eq_curves {
            return;
        }
        self.settings.eq_curves = self.model.eq_curves().clone();
        if let Err(e) = self.settings.save(&settings::default_path()) {
            log_info!("could not save settings: {e}");
        }
    }

    fn publish(&mut self) {
        let st = self.model.state();
        let linked = matches!(
            st.session,
            SessionState::Ready(_) | SessionState::Connecting
        );
        self.turned_off = still_turned_off(self.turned_off, self.linked, linked);
        self.linked = linked;
        let level = st.battery.as_ref().map(view_model::battery_level);
        let fired = self
            .monitor
            .update(level.map(|(l, _)| l), level.is_some_and(|(_, c)| c));
        let mut vm = view_model::build(st, &self.settings, self.demo, self.turned_off);
        vm.low_battery_toast = level
            .filter(|_| fired && self.settings.low_battery_notifications)
            .map(|(l, _)| l);
        (self.publish)(vm);
    }

    /// Never waits on the session: a pending Bluetooth call can hold `stop` for seconds.
    fn quit(self) {
        let Worker { model, session, .. } = self;
        drop(model);
        std::thread::spawn(move || session.stop());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_model::TileKind;
    use std::time::Instant;
    use xm_core::command::NcAmbMode;

    fn demo_engine() -> (EngineHandle, mpsc::Receiver<PanelVm>) {
        let (tx, rx) = mpsc::channel();
        let args = Args {
            demo: true,
            ..Args::default()
        };
        let h = Engine::spawn(&args, Settings::default(), move |vm| {
            let _ = tx.send(vm);
        });
        (h, rx)
    }

    fn wait_for(rx: &mpsc::Receiver<PanelVm>, ok: impl Fn(&PanelVm) -> bool) -> PanelVm {
        wait_within(rx, 15, ok)
    }

    fn wait_within(
        rx: &mpsc::Receiver<PanelVm>,
        secs: u64,
        ok: impl Fn(&PanelVm) -> bool,
    ) -> PanelVm {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let vm = rx
                .recv_timeout(left)
                .expect("timed out waiting for a view model");
            if ok(&vm) {
                return vm;
            }
        }
    }

    fn nc_on(vm: &PanelVm) -> bool {
        vm.tiles
            .iter()
            .any(|t| t.kind == TileKind::NoiseCancelling && t.on)
    }

    #[test]
    fn engine_publishes_ready_then_applies_intent() {
        let (h, rx) = demo_engine();
        let ready = wait_for(&rx, |vm| vm.connected && nc_on(vm));
        assert!(ready.status.ends_with(" · demo"));
        assert!(!ready.ambient.unwrap().enabled);

        h.send(UiCommand::Intent(Intent::SetMode(NcAmbMode::Ambient)));
        let vm = wait_for(&rx, |vm| {
            vm.tiles.iter().any(|t| t.kind == TileKind::Ambient && t.on)
        });
        assert!(vm.ambient.unwrap().enabled);
        h.send(UiCommand::Quit);
    }

    #[test]
    fn power_off_from_the_app_is_remembered_until_they_return() {
        let (h, rx) = demo_engine();
        wait_for(&rx, |vm| vm.connected);
        h.send(UiCommand::Intent(Intent::PowerOff));
        let off = wait_for(&rx, |vm| !vm.connected);
        assert!(off.status.starts_with(crate::strings::STATUS_TURNED_OFF));
        // The demo stays off for 10 s; a fresh session (Try again) reaches a new demo headset at
        // once, and the normal status returns with it.
        h.send(UiCommand::Reconnect);
        let back = wait_for(&rx, |vm| vm.connected);
        assert!(back.status.starts_with("Connected"));
        h.send(UiCommand::Quit);
    }

    #[test]
    fn turned_off_clears_when_the_headphones_come_back_by_themselves() {
        let (h, rx) = demo_engine();
        wait_for(&rx, |vm| vm.connected);
        h.send(UiCommand::Intent(Intent::PowerOff));
        wait_for(&rx, |vm| {
            vm.status.starts_with(crate::strings::STATUS_TURNED_OFF)
        });
        // The demo is off for 10 s, then the backoff (up to 8 s) finds it again.
        let back = wait_within(&rx, 30, |vm| vm.connected);
        assert!(back.status.starts_with("Connected"));
        h.send(UiCommand::Quit);
    }

    #[test]
    fn turned_off_holds_while_down_and_clears_on_return() {
        // Power-off leaves the link up for a moment: still linked, the flag holds.
        assert!(still_turned_off(true, true, true));
        // Down and retrying: holds.
        assert!(still_turned_off(true, false, false));
        // The link comes back (connecting or ready after being down): cleared.
        assert!(!still_turned_off(true, false, true));
        assert!(!still_turned_off(false, false, false));
    }

    #[test]
    fn quit_returns_promptly() {
        let (h, rx) = demo_engine();
        wait_for(&rx, |vm| vm.connected);
        let start = Instant::now();
        h.send(UiCommand::Quit);
        // The worker drops its publisher on return, which closes the channel.
        while rx.recv_timeout(Duration::from_secs(1)).is_ok() {}
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
