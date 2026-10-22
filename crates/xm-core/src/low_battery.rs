//! Low-battery edge detector: fires once when the level drops below 20 %.

const THRESHOLD: u8 = 20;

pub struct LowBatteryMonitor {
    armed: bool,
}

impl Default for LowBatteryMonitor {
    fn default() -> Self {
        Self { armed: true }
    }
}

impl LowBatteryMonitor {
    /// True once as the level falls below 20; re-arms at 20 or more, or while charging.
    /// An unknown level changes nothing.
    pub fn update(&mut self, level: Option<u8>, charging: bool) -> bool {
        let Some(level) = level else { return false };
        if charging || level >= THRESHOLD {
            self.armed = true;
            return false;
        }
        std::mem::replace(&mut self.armed, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_battery_fires_once_rearms_at_20_and_charging() {
        let mut m = LowBatteryMonitor::default();
        assert!(m.update(Some(19), false));
        assert!(!m.update(Some(18), false));
        assert!(!m.update(Some(20), false));
        assert!(m.update(Some(19), false));
        // charging re-arms, and never fires while charging
        assert!(!m.update(Some(15), true));
        assert!(m.update(Some(15), false));
        // an unknown level changes nothing: still disarmed, so no re-fire
        assert!(!m.update(None, false));
        assert!(!m.update(Some(14), false));
        assert!(!m.update(None, true));
        assert!(!m.update(Some(14), false));
    }
}
