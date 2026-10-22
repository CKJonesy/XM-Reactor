//! Take-off and put-on check: runs the session and the model for 60 seconds, printing every
//! wearing change and every effect with a local timestamp. Pause and resume use the active
//! Windows media session.

#[cfg(windows)]
fn main() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use xm_core::bluetooth::RfcommConnector;
    use xm_core::clock::SystemClock;
    use xm_core::log::timestamp;
    use xm_core::media::GsmtcMediaControl;
    use xm_core::model::{BehaviourPrefs, Effect, Model};
    use xm_core::runner::{start, SessionEvent};

    let clock = Arc::new(SystemClock);
    let handle = start(Box::new(RfcommConnector), clock.clone(), None);
    let prefs = BehaviourPrefs {
        pause_on_removal: true,
        resume_on_wear: true,
    };
    // The model's media calls block for up to 2 s, so it runs on its own thread, apart from the runner.
    let worker = std::thread::spawn(move || {
        let mut model = Model::new(Box::new(GsmtcMediaControl), prefs, clock);
        let mut wearing = model.state().wearing;
        let end = Instant::now() + Duration::from_secs(60);
        println!("{} watching for 60 s", timestamp());
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            let fx = match handle
                .events
                .recv_timeout(left.min(Duration::from_millis(100)))
            {
                Ok(SessionEvent::Connected(d)) => {
                    println!("{} connected {} ({})", timestamp(), d.name, d.address);
                    model.on_event(SessionEvent::Connected(d))
                }
                Ok(e) => model.on_event(e),
                Err(_) => model.on_tick(),
            };
            if model.state().wearing != wearing {
                wearing = model.state().wearing;
                println!("{} wearing {wearing:?}", timestamp());
            }
            for e in fx {
                println!("{} effect {e:?}", timestamp());
                if let Effect::Send(c) = e {
                    let name = c.name;
                    model.on_sent(handle.send(c), name);
                }
            }
        }
        handle.stop();
    });
    let _ = worker.join();
}

#[cfg(not(windows))]
fn main() {}
