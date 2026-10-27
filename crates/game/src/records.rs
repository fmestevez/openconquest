//! Best results: the shortest time in which each mission was won, kept in
//! `<data>/saves/best.json` (the original's own scoring is not known).

use std::collections::BTreeMap;
use std::path::Path;

/// Mission archive name to the best winning time, in game seconds.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Records(pub BTreeMap<String, f32>);

impl Records {
    /// The records in `path`, or none if it cannot be read.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self).unwrap_or_default())
    }

    /// Notes a win of `mission` in `seconds`; whether it is a new best.
    pub fn record(&mut self, mission: &str, seconds: f32) -> bool {
        match self.0.get(mission) {
            Some(&best) if best <= seconds => false,
            _ => {
                self.0.insert(mission.to_string(), seconds);
                true
            }
        }
    }

    pub fn best(&self, mission: &str) -> Option<f32> {
        self.0.get(mission).copied()
    }
}

/// `h:mm:ss` or `m:ss`.
pub fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_best_and_round_trips() {
        let mut r = Records::default();
        assert!(r.record("miss000", 600.0));
        assert!(!r.record("miss000", 700.0));
        assert!(r.record("miss000", 500.0));
        assert_eq!(r.best("miss000"), Some(500.0));
        let path =
            std::env::temp_dir().join(format!("openconquest-best-{}.json", std::process::id()));
        r.save(&path).unwrap();
        assert_eq!(Records::load(&path), r);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(65.0), "1:05");
    }
}
