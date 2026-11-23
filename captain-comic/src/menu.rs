//! The in-game settings menu (F1): toggles for the optional enhancements and key remapping.
use crate::{input::{self, Bindings}, settings::{Scaling, Settings}};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nav { Up, Down, Left, Right, Ok, Back }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row { Enhanced, Scaling, Aspect, Frame, HudSize, Fullscreen, Crt, Weather, Volume, CheatFire, CheatHealth, Keys, PadKeys, SaveGame, LoadGame, Restart, Quit }

pub const ROWS: [Row; 17] = [Row::Enhanced, Row::Scaling, Row::Aspect, Row::Frame, Row::HudSize, Row::Fullscreen, Row::Crt, Row::Weather, Row::Volume, Row::CheatFire, Row::CheatHealth, Row::Keys, Row::PadKeys, Row::SaveGame, Row::LoadGame, Row::Restart, Row::Quit];

#[derive(Default)]
pub struct Menu {
    pub sel: usize,
    /// While remapping: the action (0..6) that waits for a key.
    pub remap: Option<usize>,
    /// While remapping the controller: the action (0..4) that waits for a button.
    pub pad_remap: Option<usize>,
    /// "Restart game" was chosen once and waits for a second confirmation.
    pub confirm: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome { None, Changed, Close, Restart, Quit, Save, Load }

impl Menu {
    pub fn nav(&mut self, n: Nav, cfg: &mut Settings, bindings: &mut Bindings) -> Outcome {
        let _ = bindings;
        if self.pad_remap.is_some() { return Outcome::None; } // the host feeds buttons through `pad_button`
        if self.remap.is_some() { if n == Nav::Back { self.remap = None; } return Outcome::None; }
        let was_confirming = std::mem::take(&mut self.confirm);
        match n {
            Nav::Up => { self.sel = (self.sel + ROWS.len() - 1) % ROWS.len(); Outcome::None }
            Nav::Down => { self.sel = (self.sel + 1) % ROWS.len(); Outcome::None }
            Nav::Back => Outcome::Close,
            Nav::Left | Nav::Right | Nav::Ok => match ROWS[self.sel] {
                Row::Enhanced => { let on = cfg.enhanced_count() < crate::settings::ENHANCED_TOTAL; cfg.set_enhanced(on); Outcome::Changed }
                Row::Scaling => { cfg.scaling = if cfg.scaling == Scaling::Fit { Scaling::Integer } else { Scaling::Fit }; Outcome::Changed }
                Row::Aspect => {
                    cfg.widescreen = !cfg.widescreen;
                    Outcome::Changed
                }
                Row::Weather => { cfg.weather = if cfg.weather == crate::settings::Weather::Off { crate::settings::Weather::Rain } else { crate::settings::Weather::Off }; Outcome::Changed }
                Row::HudSize => { cfg.hud_size = match n { Nav::Left => cfg.hud_size.saturating_sub(1), Nav::Right => (cfg.hud_size + 1).min(2), _ => (cfg.hud_size + 1) % 3 }; Outcome::Changed }
                Row::Frame => { cfg.immersive = !cfg.immersive; Outcome::Changed }
                Row::Fullscreen => { cfg.fullscreen = !cfg.fullscreen; Outcome::Changed }
                Row::Crt => { cfg.crt = !cfg.crt; Outcome::Changed }
                Row::CheatFire => { cfg.cheat_fire = !cfg.cheat_fire; Outcome::Changed }
                Row::CheatHealth => { cfg.cheat_health = !cfg.cheat_health; Outcome::Changed }
                Row::Restart => if n == Nav::Ok { if was_confirming { Outcome::Restart } else { self.confirm = true; Outcome::None } } else { Outcome::None },
                Row::Quit => if n == Nav::Ok { if was_confirming { Outcome::Quit } else { self.confirm = true; Outcome::None } } else { Outcome::None },
                Row::Keys => { if n == Nav::Ok { self.remap = Some(0); } Outcome::None }
                Row::PadKeys => { if n == Nav::Ok { self.pad_remap = Some(0); } Outcome::None }
                Row::Volume => {
                    cfg.volume = match n { Nav::Left => cfg.volume.saturating_sub(1), Nav::Right => (cfg.volume + 1).min(10), _ => if cfg.volume >= 10 { 0 } else { cfg.volume + 1 } };
                    Outcome::Changed
                }
                Row::SaveGame => if n == Nav::Ok { Outcome::Save } else { Outcome::None },
                Row::LoadGame => if n == Nav::Ok { Outcome::Load } else { Outcome::None },
            },
        }
    }

    /// A key was pressed while remapping. Returns true when the whole set is done (and should be saved).
    pub fn remap_key(&mut self, sc: Option<u8>, bindings: &mut Bindings) -> bool {
        let Some(a) = self.remap else { return false };
        let Some(sc) = sc.filter(|&s| input::assignable(s)) else { return false };
        if (0..a).any(|i| bindings.get(i) == sc) { return false; } // already used by an earlier action
        bindings.set(a, sc);
        if a + 1 >= Bindings::ACTIONS.len() { self.remap = None; true } else { self.remap = Some(a + 1); false }
    }

    /// A controller button was pressed while remapping. Returns true when the whole set is done (and should be saved).
    pub fn pad_button(&mut self, b: gilrs::Button, map: &mut [gilrs::Button; 4]) -> bool {
        let Some(a) = self.pad_remap else { return false };
        if map[..a].contains(&b) { return false; } // already used by an earlier action
        map[a] = b;
        if a + 1 >= crate::pad::ACTIONS.len() { self.pad_remap = None; true } else { self.pad_remap = Some(a + 1); false }
    }

    pub fn title(&self) -> &'static str { "Settings" }

    /// The key reference shown beside the menu (`label|value` rows).
    pub fn key_lines(b: &Bindings, pad: &[gilrs::Button; 4]) -> Vec<String> {
        let mut v = vec!["Keys||Pad".to_string()];
        // the controller column: d-pad moves, then jump, fire, open door, teleport as mapped
        let pads = ["D-pad".to_string(), "D-pad".to_string(), crate::pad::button_name(pad[0]).to_string(), crate::pad::button_name(pad[1]).to_string(),
                    format!("{} / Dn", crate::pad::button_name(pad[2])), crate::pad::button_name(pad[3]).to_string()];
        for (i, name) in Bindings::ACTIONS.iter().enumerate() { v.push(format!("{}|{}|{}", name, input::name(b.get(i)), pads[i])); }
        v.push("WASD also moves|".to_string());
        for (a, d) in [("Esc", "Pause (Q quits)"), ("F1 / Start", "This menu"), ("Select", "Enhanced on/off"), ("F2", "Sound"), 
                       ("F5 / F9", "Save / load"), ("F11", "Fullscreen")] {
            v.push(format!("{a}|{d}"));
        }
        v
    }

    /// One string per row; `label|value` rows are drawn in two columns.
    pub fn lines(&self, cfg: &Settings, b: &Bindings) -> Vec<String> {
        let onoff = |v: bool| if v { "On" } else { "Off" };
        if let Some(a) = self.remap {
            let mut v = vec!["Press a key for each action".to_string(), "(Esc cancels)".to_string(), String::new()];
            for (i, name) in Bindings::ACTIONS.iter().enumerate() {
                let key = if i < a { input::name(b.get(i)) } else if i == a { "...".to_string() } else { String::new() };
                v.push(format!("{}{}|{}", if i == a { "> " } else { "  " }, name, key));
            }
            return v;
        }
        if let Some(a) = self.pad_remap {
            let mut v = vec!["Press a button for each action".to_string(), "(Start or Esc cancels)".to_string(), String::new()];
            for (i, name) in crate::pad::ACTIONS.iter().enumerate() {
                let key = if i < a { crate::pad::button_name(cfg.pad[i]).to_string() } else if i == a { "...".to_string() } else { String::new() };
                v.push(format!("{}{}|{}", if i == a { "> " } else { "  " }, name, key));
            }
            return v;
        }
        ROWS.iter().enumerate().map(|(i, r)| {
            let text = match r {
                Row::Enhanced => format!("Enhanced|{}", match cfg.enhanced_count() { 0 => "Off", crate::settings::ENHANCED_TOTAL => "On", _ => "Custom" }),
                Row::Scaling => format!("Scaling|{}", if cfg.scaling == Scaling::Fit { "Fit window" } else { "Integer (sharp)" }),
                Row::Aspect => format!("Aspect ratio|{}", if cfg.widescreen { "16:9" } else { "4:3" }),
                Row::Weather => format!("Weather|{}", match cfg.weather { crate::settings::Weather::Off => "Off", crate::settings::Weather::Rain => "Rain" }),
                Row::HudSize => format!("HUD size|{}", ["Small", "Medium", "Large"][cfg.hud_size.min(2) as usize]),
                Row::Frame => format!("Window frame|{}", if cfg.immersive { "Immersive" } else { "Classic" }),
                Row::Fullscreen => format!("Fullscreen|{}", onoff(cfg.fullscreen)),
                Row::Crt => format!("CRT filter|{}", onoff(cfg.crt)),
                Row::Keys => "Remap keys...".to_string(),
                Row::PadKeys => "Remap controller...".to_string(),
                Row::Volume => format!("Volume|{}%", cfg.volume as u32 * 10),
                Row::CheatFire => format!("Infinite fireballs|{}", onoff(cfg.cheat_fire)),
                Row::CheatHealth => format!("Infinite health|{}", onoff(cfg.cheat_health)),
                Row::Restart => if self.confirm && ROWS[self.sel] == Row::Restart { "Restart? Press Enter".to_string() } else { "Restart game".to_string() },
                Row::Quit => if self.confirm && ROWS[self.sel] == Row::Quit { "Quit? Press Enter".to_string() } else { "Quit game".to_string() },
                Row::SaveGame => "Save game".to_string(),
                Row::LoadGame => "Load game".to_string(),
            };
            format!("{}{}", if i == self.sel { "> " } else { "  " }, text)
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_toggle_and_wrap() {
        let (mut m, mut c, mut b) = (Menu::default(), Settings::default(), Bindings::DEFAULT);
        assert_eq!(m.nav(Nav::Up, &mut c, &mut b), Outcome::None);
        assert_eq!(ROWS[m.sel], Row::Quit, "wraps to the last row");
        m.nav(Nav::Down, &mut c, &mut b);
        assert_eq!(ROWS[m.sel], Row::Enhanced);
        assert_eq!(m.nav(Nav::Ok, &mut c, &mut b), Outcome::Changed);
        assert!(c.smooth && c.effects && !c.widescreen, "the bundle is on, without widescreen");
        assert_eq!(m.nav(Nav::Ok, &mut c, &mut b), Outcome::Changed);
        assert_eq!(c.enhanced_count(), 0, "all on -> pressing again turns the bundle off");
        m.nav(Nav::Down, &mut c, &mut b);
        m.nav(Nav::Right, &mut c, &mut b);
        assert_eq!(c.scaling, Scaling::Integer, "Right toggles Fit -> Integer (Fit is the default)");
        assert_eq!(m.nav(Nav::Back, &mut c, &mut b), Outcome::Close);
    }

    #[test]
    fn aspect_ratio_cycles_between_4_3_and_16_9_repeatedly() {
        let (mut m, mut c, mut b) = (Menu::default(), Settings::default(), Bindings::DEFAULT);
        m.sel = ROWS.iter().position(|r| *r == Row::Aspect).unwrap();
        for _ in 0..3 {
            m.nav(Nav::Ok, &mut c, &mut b);
            assert!(c.widescreen);
            m.nav(Nav::Right, &mut c, &mut b);
            assert!(!c.widescreen);
        }
    }

    #[test]
    fn remapping_collects_six_distinct_keys() {
        let (mut m, mut c, mut b) = (Menu::default(), Settings::default(), Bindings::DEFAULT);
        m.sel = ROWS.iter().position(|r| *r == Row::Keys).unwrap();
        m.nav(Nav::Ok, &mut c, &mut b);
        assert_eq!(m.remap, Some(0));
        assert!(!m.remap_key(Some(0x01), &mut b), "Esc cannot be bound");
        let keys = [0x1e, 0x20, 0x11, 0x21, 0x1f, 0x14];
        for (i, k) in keys.iter().enumerate() {
            assert!(!m.remap_key(Some(*k), &mut b) || i == 5);
            if i < 5 { assert!(!m.remap_key(Some(*k), &mut b), "a key used already is ignored"); }
        }
        assert_eq!(m.remap, None);
        assert_eq!((b.left, b.right, b.jump, b.fire, b.open, b.teleport), (0x1e, 0x20, 0x11, 0x21, 0x1f, 0x14));
    }

    #[test]
    fn quit_and_restart_need_a_second_press() {
        let (mut m, mut c, mut b) = (Menu::default(), Settings::default(), Bindings::DEFAULT);
        m.sel = ROWS.iter().position(|r| *r == Row::Quit).unwrap();
        assert_eq!(m.nav(Nav::Ok, &mut c, &mut b), Outcome::None);
        assert_eq!(m.nav(Nav::Ok, &mut c, &mut b), Outcome::Quit);
        m.sel = ROWS.iter().position(|r| *r == Row::Quit).unwrap();
        m.nav(Nav::Ok, &mut c, &mut b);
        m.nav(Nav::Up, &mut c, &mut b); // moving away cancels the confirmation
        m.nav(Nav::Down, &mut c, &mut b);
        assert_eq!(m.nav(Nav::Ok, &mut c, &mut b), Outcome::None);
    }

    #[test]
    fn volume_steps_and_the_controller_remap_collects_four_distinct_buttons() {
        use gilrs::Button::*;
        let (mut m, mut c, mut b) = (Menu::default(), Settings::default(), Bindings::DEFAULT);
        m.sel = ROWS.iter().position(|r| *r == Row::Volume).unwrap();
        m.nav(Nav::Left, &mut c, &mut b);
        assert_eq!(c.volume, 4);
        for _ in 0..20 { m.nav(Nav::Right, &mut c, &mut b); }
        assert_eq!(c.volume, 10, "Left/Right stop at the ends");
        m.nav(Nav::Ok, &mut c, &mut b);
        assert_eq!(c.volume, 0, "Enter wraps around");
        m.sel = ROWS.iter().position(|r| *r == Row::PadKeys).unwrap();
        m.nav(Nav::Ok, &mut c, &mut b);
        assert_eq!(m.pad_remap, Some(0));
        let mut map = c.pad;
        assert!(!m.pad_button(RightTrigger, &mut map));
        assert!(!m.pad_button(RightTrigger, &mut map), "a button used already is ignored");
        assert!(!m.pad_button(South, &mut map));
        assert!(!m.pad_button(East, &mut map));
        assert!(m.pad_button(LeftTrigger, &mut map));
        assert_eq!(map, [RightTrigger, South, East, LeftTrigger]);
        assert_eq!(m.pad_remap, None);
    }

    #[test]
    fn remapping_can_be_cancelled() {
        let (mut m, mut c, mut b) = (Menu::default(), Settings::default(), Bindings::DEFAULT);
        m.remap = Some(2);
        m.nav(Nav::Back, &mut c, &mut b);
        assert_eq!(m.remap, None);
    }
}
