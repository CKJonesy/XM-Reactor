//! Connects to the paired headset and prints every session event for 20 seconds.

#[cfg(windows)]
fn main() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use xm_core::bluetooth::RfcommConnector;
    use xm_core::clock::SystemClock;
    use xm_core::runner::{start, SessionEvent};

    let handle = start(Box::new(RfcommConnector), Arc::new(SystemClock), None);
    let end = Instant::now() + Duration::from_secs(20);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match handle.events.recv_timeout(left) {
            Ok(SessionEvent::Connected(d)) => println!("Connected {} ({})", d.name, d.address),
            Ok(e) => println!("{e:?}"),
            Err(_) => break,
        }
    }
    handle.stop();
}

#[cfg(not(windows))]
fn main() {}
