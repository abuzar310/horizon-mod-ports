//! Speedometer: how fast you move, in a corner of the screen. The speed is measured over the last
//! second from where you were each client tick, so it shows walking, sprinting, riding, boats and
//! elytra flight alike: blocks per second (a block is a metre) or km/h, across the ground by
//! default, or with climbing and falling counted too.
use std::collections::VecDeque;
use std::sync::Mutex;

use mod_api::{
    ClientHost, ClientTickPostEvent, Corner, Hooks, Registrar, RegistrationContext,
    RenderGuiPostEvent, SettingSpec,
};

/// The mod's id: its settings folder and translation keys.
pub const ID: &str = "speedometer";
/// The WebAssembly mods' panel, as Coordinates HUD draws it: translucent black, white text.
pub const BACKGROUND: u32 = 0x7800_0000;
pub const INK: u32 = 0xFFFF_FFFF;
/// A line's height and the text's inset, in GUI pixels.
pub const LINE: f32 = 10.0;
pub const PADDING: f32 = 2.0;
/// How far back the speed looks, in milliseconds: one second.
pub const WINDOW_MS: u64 = 1000;
/// Farther than this between two ticks is a teleport, a respawn or another world, not movement:
/// the measurement starts again. Elytra flight with rockets stays well under it.
pub const JUMP: f64 = 16.0;
/// The corners in the order the corner setting lists them.
pub const CORNERS: [Corner; 4] = Corner::ALL;

/// The settings on Speedometer's page in Options > Mods.
pub fn settings() -> [SettingSpec; 4] {
    [
        SettingSpec::toggle("shown", "speedometer.setting.shown", true)
            .tooltip("speedometer.setting.shown.tooltip"),
        SettingSpec::choice(
            "corner",
            "speedometer.setting.corner",
            CORNERS.map(|c| format!("speedometer.corner.{}", c.name())),
            3,
        )
        .tooltip("speedometer.setting.corner.tooltip"),
        SettingSpec::choice(
            "unit",
            "speedometer.setting.unit",
            ["speedometer.unit.blocks", "speedometer.unit.kmh"],
            0,
        )
        .tooltip("speedometer.setting.unit.tooltip"),
        SettingSpec::toggle("vertical", "speedometer.setting.vertical", false)
            .tooltip("speedometer.setting.vertical.tooltip"),
    ]
}

/// Where the player was, and when (the game's clock, in milliseconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: u64,
    pub pos: [f64; 3],
}

/// The samples of the last [`WINDOW_MS`], oldest first.
#[derive(Debug, Default)]
pub struct Track {
    samples: VecDeque<Sample>,
}

impl Track {
    /// Adds where the player is now. A jump farther than [`JUMP`] since the last sample, or a
    /// clock that went back, starts the measurement again from here.
    pub fn push(&mut self, sample: Sample) {
        if let Some(last) = self.samples.back()
            && (sample.at < last.at || distance(last.pos, sample.pos, true) > JUMP)
        {
            self.samples.clear();
        }
        self.samples.push_back(sample);
        // Keep one sample at least a window old, so the speed always spans a whole window.
        while self.samples.len() > 2 && sample.at - self.samples[1].at >= WINDOW_MS {
            self.samples.pop_front();
        }
    }

    /// The speed in blocks per second over the samples kept: across the ground, or with height
    /// changes too when `vertical`. Zero until two samples span a tenth of a second.
    pub fn speed(&self, vertical: bool) -> f64 {
        let (Some(first), Some(last)) = (self.samples.front(), self.samples.back()) else {
            return 0.0;
        };
        let elapsed = last.at.saturating_sub(first.at);
        if elapsed < 100 {
            return 0.0;
        }
        let path: f64 = self
            .samples
            .iter()
            .zip(self.samples.iter().skip(1))
            .map(|(a, b)| distance(a.pos, b.pos, vertical))
            .sum();
        path * 1000.0 / elapsed as f64
    }

    /// Forgets every sample (leaving a world).
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

/// The distance between two positions, across the ground only unless `vertical`.
pub fn distance(a: [f64; 3], b: [f64; 3], vertical: bool) -> f64 {
    let dx = b[0] - a[0];
    let dy = if vertical { b[1] - a[1] } else { 0.0 };
    let dz = b[2] - a[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// The line the panel shows: `4.3 blocks/s` or `15.5 km/h` (option 1).
pub fn line(blocks_per_second: f64, unit: usize) -> String {
    match unit {
        1 => format!("{:.1} km/h", blocks_per_second * 3.6),
        _ => format!("{:.1} blocks/s", blocks_per_second),
    }
}

/// The panel around a line `width` GUI pixels wide: `[w, h]`.
pub fn size(width: f32) -> [f32; 2] {
    [width + 2.0 * PADDING, LINE + PADDING]
}

#[derive(Default)]
struct Speedometer {
    track: Mutex<Track>,
}

impl Speedometer {
    fn track(&self) -> std::sync::MutexGuard<'_, Track> {
        self.track.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[mod_api::horizon_mod]
impl Hooks for Speedometer {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// One sample a client tick, where the player is: none outside a world.
    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        let view: &dyn ClientHost = &*host;
        let position = view
            .local_player()
            .and_then(|player| player.entity().position(view));
        let at = view.millis();
        let mut track = self.track();
        match position {
            Some(pos) => track.push(Sample { at, pos }),
            None => track.clear(),
        }
    }

    /// The panel in the corner picked, below what the corner holds already; none while F3's
    /// debug screen takes that corner.
    fn render_gui_post(&self, host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
        if !host.on(ID) || !host.setting_bool("shown").unwrap_or(true) {
            return;
        }
        let corner = host
            .setting_choice("corner")
            .map_or(3, |i| i as usize)
            .min(CORNERS.len() - 1);
        let corner = CORNERS[corner];
        if !host.native_corner_shown(corner) || host.local_player().is_none() {
            return;
        }
        let vertical = host.setting_bool("vertical").unwrap_or(false);
        let unit = host.setting_choice("unit").map_or(0, |i| i as usize);
        let text = line(self.track().speed(vertical), unit);
        let draw = &mut *event.draw;
        let [w, h] = size(draw.text_width(&text));
        let (x, y) = event.corners.place(corner, w, h);
        draw.fill([x, y, w, h], BACKGROUND);
        draw.text(&text, [x + PADDING, y + PADDING], INK, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64, x: f64, y: f64, z: f64) -> Sample {
        Sample {
            at: ms,
            pos: [x, y, z],
        }
    }

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
    }

    #[test]
    fn walking_speed_over_the_last_second() {
        // Sprinting covers about 5.6 blocks a second: 0.28 a tick.
        let mut track = Track::default();
        for tick in 0..=40u64 {
            track.push(at(tick * 50, tick as f64 * 0.28, 64.0, 0.0));
        }
        assert!(
            (track.speed(false) - 5.6).abs() < 1e-9,
            "{}",
            track.speed(false)
        );
        assert_eq!(line(track.speed(false), 0), "5.6 blocks/s");
        assert_eq!(line(track.speed(false), 1), "20.2 km/h");
    }

    #[test]
    fn falling_counts_only_when_asked() {
        let mut track = Track::default();
        for tick in 0..=20u64 {
            track.push(at(tick * 50, 0.0, 100.0 - tick as f64, 0.0));
        }
        assert_eq!(track.speed(false), 0.0);
        assert!((track.speed(true) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_teleport_starts_again() {
        let mut track = Track::default();
        for tick in 0..=10u64 {
            track.push(at(tick * 50, tick as f64 * 0.2, 64.0, 0.0));
        }
        track.push(at(550, 5000.0, 64.0, 0.0));
        assert_eq!(track.speed(false), 0.0, "one sample after the jump");
        track.push(at(700, 5000.6, 64.0, 0.0));
        assert!((track.speed(false) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn only_the_last_second_counts() {
        let mut track = Track::default();
        // A second of sprinting, then a second standing still.
        for tick in 0..=20u64 {
            track.push(at(tick * 50, tick as f64 * 0.28, 64.0, 0.0));
        }
        for tick in 21..=40u64 {
            track.push(at(tick * 50, 20.0 * 0.28, 64.0, 0.0));
        }
        assert!(track.speed(false) < 0.3, "{}", track.speed(false));
    }

    #[test]
    fn too_short_a_track_reads_zero() {
        let mut track = Track::default();
        assert_eq!(track.speed(false), 0.0);
        track.push(at(0, 0.0, 64.0, 0.0));
        track.push(at(50, 1.0, 64.0, 0.0));
        assert_eq!(track.speed(false), 0.0);
        track.clear();
        assert_eq!(track.speed(false), 0.0);
    }

    #[test]
    fn the_panel_fits_its_line() {
        assert_eq!(size(60.0), [64.0, 12.0]);
        assert_eq!(CORNERS[3], Corner::BottomRight);
    }
}
