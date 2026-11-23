//! Optional enhancements. Everything defaults to the faithful original behaviour except the window, which starts
//! fullscreen and fitted to the screen; changes are stored in
//! `comic.cfg` (simple `key=value` lines) in the working directory.
use std::{fs, path::Path};

pub const FILE: &str = "comic.cfg";
/// How many settings the Enhanced bundle holds.
pub const ENHANCED_TOTAL: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scaling { Integer, Fit }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Weather { Off, Rain }

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Interpolate movement between the ~9 Hz game ticks (game logic is unchanged).
    pub smooth: bool,
    /// Integer scaling keeps pixels sharp; Fit fills the window.
    pub scaling: Scaling,
    pub fullscreen: bool,
    /// Show a message when a door needs the Door Key.
    pub hints: bool,
    /// Show extra playfield to the left and right of the original window.
    pub widescreen: bool,
    /// CRT look: scanlines, soft phosphor, slight curvature and vignette (shader).
    pub crt: bool,
    /// Particles, glow, soft shadow, hit flash and shake, fades between stages, score count-up.
    pub effects: bool,
    /// Rain over the outdoor levels (needs Effects to be visible).
    pub weather: Weather,
    /// No window frame: the world fills the window and the score, bars and lives are drawn over it.
    pub immersive: bool,
    /// Size of the immersive HUD: 0 small, 1 medium, 2 large.
    pub hud_size: u8,
    /// Controller rumble when Comic is hurt or dies.
    pub rumble: bool,
    /// Sound volume, 0..=10 (5 is the original loudness).
    pub volume: u8,
    /// Controller buttons for jump, fire, open door and teleport.
    pub pad: [gilrs::Button; 4],
    /// The cheats are saved with the other settings.
    pub cheat_fire: bool,
    pub cheat_health: bool,
}

impl Default for Settings {
    fn default() -> Settings { Settings { smooth: false, scaling: Scaling::Fit, fullscreen: true, hints: false, widescreen: false, crt: false, effects: false, weather: Weather::Off, immersive: false, hud_size: 1, rumble: false, volume: 5, pad: crate::pad::DEFAULT_MAP, cheat_fire: false, cheat_health: false } }
}

impl Settings {
    /// "Enhanced" is a bundle of the enhancements that change how the game looks and plays smoothly (not the CRT
    /// filter, fullscreen or the cheats): smooth motion, effects, controller rumble and the door hint (the aspect ratio stays as chosen).
    pub fn enhanced_count(&self) -> usize {
        [self.smooth, self.effects, self.rumble, self.hints].iter().filter(|b| **b).count()
    }

    pub fn set_enhanced(&mut self, on: bool) {
        self.smooth = on;
        self.effects = on;
        self.rumble = on;
        self.hints = on;
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for l in text.lines() {
            let Some((k, v)) = l.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let b = matches!(v, "1" | "true" | "on" | "yes");
            match k {
                "smooth" => s.smooth = b,
                "scaling" => s.scaling = if v == "fit" { Scaling::Fit } else { Scaling::Integer },
                "fullscreen" => s.fullscreen = b,
                "hints" => s.hints = b,
                "widescreen" => s.widescreen = b,
                "crt" => s.crt = b,
                "effects" => s.effects = b,
                "weather" => s.weather = if v == "rain" { Weather::Rain } else { Weather::Off },
                "immersive" => s.immersive = b,
                "hud" => s.hud_size = v.parse::<u8>().map_or(1, |x| x.min(2)),
                "rumble" => s.rumble = b,
                "cheat_fire" => s.cheat_fire = b,
                "cheat_health" => s.cheat_health = b,
                "volume" => s.volume = v.parse::<u8>().map_or(5, |x| x.min(10)),
                "pad_jump" | "pad_fire" | "pad_open" | "pad_teleport" => {
                    let i = ["pad_jump", "pad_fire", "pad_open", "pad_teleport"].iter().position(|n| *n == k).unwrap();
                    if let Some(btn) = crate::pad::button_from_name(v) { s.pad[i] = btn; }
                }
                _ => {}
            }
        }
        s
    }

    pub fn to_text(&self) -> String {
        let on = |b: bool| if b { 1 } else { 0 };
        format!("smooth={}\nscaling={}\nfullscreen={}\nhints={}\nwidescreen={}\ncrt={}\neffects={}\nweather={}\nimmersive={}\nhud={}\nrumble={}\ncheat_fire={}\ncheat_health={}\nvolume={}\npad_jump={}\npad_fire={}\npad_open={}\npad_teleport={}\n", on(self.smooth),
            if self.scaling == Scaling::Fit { "fit" } else { "integer" }, on(self.fullscreen), on(self.hints), on(self.widescreen), on(self.crt),
            on(self.effects), match self.weather { Weather::Off => "off", Weather::Rain => "rain" }, on(self.immersive), self.hud_size, on(self.rumble), on(self.cheat_fire), on(self.cheat_health), self.volume, crate::pad::button_name(self.pad[0]), crate::pad::button_name(self.pad[1]), crate::pad::button_name(self.pad[2]), crate::pad::button_name(self.pad[3]))
    }

    pub fn load() -> Settings { fs::read_to_string(Path::new(FILE)).map(|t| Settings::parse(&t)).unwrap_or_default() }
    pub fn save(&self) { let _ = fs::write(FILE, self.to_text()); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enhanced_bundle_switches_everything_in_it() {
        let mut s = Settings::default();
        assert_eq!(s.enhanced_count(), 0);
        s.set_enhanced(true);
        assert_eq!(s.enhanced_count(), ENHANCED_TOTAL);
        assert!(s.smooth && !s.widescreen && s.rumble && s.effects && s.hints && !s.crt);
        s.set_enhanced(false);
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn defaults_are_faithful_and_round_trip() {
        let d = Settings::default();
        assert!(!d.smooth && !d.hints && !d.widescreen && !d.crt && !d.effects && d.fullscreen && d.scaling == Scaling::Fit);
        let s = Settings { smooth: true, scaling: Scaling::Fit, fullscreen: true, hints: true, widescreen: true, crt: true, effects: true, weather: Weather::Rain, immersive: true, hud_size: 2, rumble: true, volume: 8, pad: [gilrs::Button::RightTrigger, gilrs::Button::South, gilrs::Button::East, gilrs::Button::LeftTrigger], cheat_fire: true, cheat_health: true };
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(Settings::parse("garbage\nsmooth=on\n"), Settings { smooth: true, ..d });
    }
}
