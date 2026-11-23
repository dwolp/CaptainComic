//! Key bindings stored as original DOS scancodes (KEYS.DEF format: 6 bytes = jump, fire, left, right, open, teleport).
use macroquad::prelude::KeyCode;
use std::{fs, io, path::Path};

/// DOS scancode (set 1 make code) <-> macroquad key. Keypad twins are handled in `is_down`.
const TABLE: &[(u8, KeyCode)] = &[
    (0x01, KeyCode::Escape), (0x02, KeyCode::Key1), (0x03, KeyCode::Key2), (0x04, KeyCode::Key3), (0x05, KeyCode::Key4),
    (0x06, KeyCode::Key5), (0x07, KeyCode::Key6), (0x08, KeyCode::Key7), (0x09, KeyCode::Key8), (0x0a, KeyCode::Key9),
    (0x0b, KeyCode::Key0), (0x0c, KeyCode::Minus), (0x0d, KeyCode::Equal), (0x0e, KeyCode::Backspace), (0x0f, KeyCode::Tab),
    (0x10, KeyCode::Q), (0x11, KeyCode::W), (0x12, KeyCode::E), (0x13, KeyCode::R), (0x14, KeyCode::T), (0x15, KeyCode::Y),
    (0x16, KeyCode::U), (0x17, KeyCode::I), (0x18, KeyCode::O), (0x19, KeyCode::P), (0x1a, KeyCode::LeftBracket),
    (0x1b, KeyCode::RightBracket), (0x1c, KeyCode::Enter), (0x1d, KeyCode::LeftControl), (0x1e, KeyCode::A),
    (0x1f, KeyCode::S), (0x20, KeyCode::D), (0x21, KeyCode::F), (0x22, KeyCode::G), (0x23, KeyCode::H), (0x24, KeyCode::J),
    (0x25, KeyCode::K), (0x26, KeyCode::L), (0x27, KeyCode::Semicolon), (0x28, KeyCode::Apostrophe), (0x29, KeyCode::GraveAccent),
    (0x2a, KeyCode::LeftShift), (0x2b, KeyCode::Backslash), (0x2c, KeyCode::Z), (0x2d, KeyCode::X), (0x2e, KeyCode::C),
    (0x2f, KeyCode::V), (0x30, KeyCode::B), (0x31, KeyCode::N), (0x32, KeyCode::M), (0x33, KeyCode::Comma),
    (0x34, KeyCode::Period), (0x35, KeyCode::Slash), (0x36, KeyCode::RightShift), (0x37, KeyCode::KpMultiply),
    (0x38, KeyCode::LeftAlt), (0x39, KeyCode::Space), (0x3a, KeyCode::CapsLock),
    (0x3b, KeyCode::F1), (0x3c, KeyCode::F2), (0x3d, KeyCode::F3), (0x3e, KeyCode::F4), (0x3f, KeyCode::F5),
    (0x40, KeyCode::F6), (0x41, KeyCode::F7), (0x42, KeyCode::F8), (0x43, KeyCode::F9), (0x44, KeyCode::F10),
    (0x45, KeyCode::NumLock), (0x46, KeyCode::ScrollLock), (0x47, KeyCode::Home), (0x48, KeyCode::Up), (0x49, KeyCode::PageUp),
    (0x4a, KeyCode::KpSubtract), (0x4b, KeyCode::Left), (0x4d, KeyCode::Right), (0x4e, KeyCode::KpAdd),
    (0x4f, KeyCode::End), (0x50, KeyCode::Down), (0x51, KeyCode::PageDown), (0x52, KeyCode::Insert), (0x53, KeyCode::Delete),
];

/// Keys that share a scancode with a primary key (right-hand modifiers, numeric keypad).
const TWINS: &[(u8, KeyCode)] = &[
    (0x1d, KeyCode::RightControl), (0x38, KeyCode::RightAlt),
    (0x47, KeyCode::Kp7), (0x48, KeyCode::Kp8), (0x49, KeyCode::Kp9), (0x4b, KeyCode::Kp4), (0x4c, KeyCode::Kp5),
    (0x4d, KeyCode::Kp6), (0x4f, KeyCode::Kp1), (0x50, KeyCode::Kp2), (0x51, KeyCode::Kp3), (0x52, KeyCode::Kp0),
    (0x53, KeyCode::KpDecimal), (0x1c, KeyCode::KpEnter),
];

pub fn scancode(k: KeyCode) -> Option<u8> {
    TABLE.iter().chain(TWINS.iter()).find(|(_, kc)| *kc == k).map(|(s, _)| *s)
}

pub fn is_down(sc: u8) -> bool {
    use macroquad::prelude::is_key_down;
    TABLE.iter().chain(TWINS.iter()).any(|(s, k)| *s == sc && is_key_down(*k))
}

/// A scancode the original accepts when defining keys (not Esc, within its 83-entry name table).
pub fn assignable(sc: u8) -> bool { (0x02..=0x53).contains(&sc) }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bindings { pub jump: u8, pub fire: u8, pub left: u8, pub right: u8, pub open: u8, pub teleport: u8 }

impl Bindings {
    /// The original's defaults: Space jump, Insert fire, arrows move, Alt open door, Caps Lock teleport.
    pub const DEFAULT: Bindings = Bindings { jump: 0x39, fire: 0x52, left: 0x4b, right: 0x4d, open: 0x38, teleport: 0x3a };

    /// Actions in the order the original's key dialog asks for them.
    pub const ACTIONS: [&'static str; 6] = ["Left", "Right", "Jump", "Fire", "Open door", "Teleport"];

    pub fn get(&self, action: usize) -> u8 { [self.left, self.right, self.jump, self.fire, self.open, self.teleport][action] }

    pub fn set(&mut self, action: usize, sc: u8) {
        match action { 0 => self.left = sc, 1 => self.right = sc, 2 => self.jump = sc, 3 => self.fire = sc, 4 => self.open = sc, _ => self.teleport = sc }
    }

    pub fn to_bytes(&self) -> [u8; 6] { [self.jump, self.fire, self.left, self.right, self.open, self.teleport] }

    pub fn from_bytes(b: &[u8]) -> Option<Bindings> {
        if b.len() < 6 || !b[..6].iter().all(|&s| assignable(s)) { return None; }
        Some(Bindings { jump: b[0], fire: b[1], left: b[2], right: b[3], open: b[4], teleport: b[5] })
    }

    /// Read KEYS.DEF from the working directory, then the asset directory; fall back to the defaults.
    pub fn load(asset_dir: &Path) -> Bindings {
        for p in [Path::new("KEYS.DEF").to_path_buf(), asset_dir.join("KEYS.DEF")] {
            if let Some(b) = fs::read(&p).ok().and_then(|d| Bindings::from_bytes(&d)) { return b; }
        }
        Bindings::DEFAULT
    }

    pub fn save(&self) -> io::Result<()> { fs::write("KEYS.DEF", self.to_bytes()) }
}

/// A readable name for a scancode (for the settings menu).
pub fn name(sc: u8) -> String {
    TABLE.iter().find(|(s, _)| *s == sc).map_or("?".to_string(), |(_, k)| format!("{k:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_and_set_use_the_dialog_order() {
        let mut b = Bindings::DEFAULT;
        assert_eq!((b.get(0), b.get(2), b.get(5)), (0x4b, 0x39, 0x3a));
        b.set(3, 0x21);
        assert_eq!(b.fire, 0x21);
        assert_eq!(name(0x39), "Space");
    }

    #[test]
    fn defaults_match_the_originals_keys_def() {
        // the original's initial bytes at CS:0x19.. are 39 52 4b 4d 38 3a (jump, fire, left, right, open, teleport)
        assert_eq!(Bindings::DEFAULT.to_bytes(), [0x39, 0x52, 0x4b, 0x4d, 0x38, 0x3a]);
        assert_eq!(Bindings::from_bytes(&Bindings::DEFAULT.to_bytes()), Some(Bindings::DEFAULT));
    }

    #[test]
    fn scancodes_round_trip_and_reject_what_the_original_rejects() {
        assert_eq!(scancode(KeyCode::Space), Some(0x39));
        assert_eq!(scancode(KeyCode::Left), Some(0x4b));
        assert_eq!(scancode(KeyCode::RightAlt), Some(0x38));
        assert!(!assignable(0x01), "Esc cannot be bound");
        assert!(!assignable(0x54));
        assert!(Bindings::from_bytes(&[0x39, 0x52, 0x4b, 0x4d, 0x38]).is_none(), "too short");
        assert!(Bindings::from_bytes(&[0x39, 0x01, 0x4b, 0x4d, 0x38, 0x3a]).is_none(), "contains Esc");
        for (sc, k) in TABLE { assert_eq!(scancode(*k), Some(*sc), "{k:?}"); }
    }
}
