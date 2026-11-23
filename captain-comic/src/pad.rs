//! Optional gamepad support (XInput and others via gilrs): d-pad / left stick move, A jump, X fire, B or d-pad down
//! open door, Y teleport, Start opens the settings menu, Select/Back toggles Enhanced. Menu navigation uses the d-pad, A (select) and B (back).
use crate::game::Input;
use gilrs::ff::{BaseEffect, BaseEffectType, Effect, EffectBuilder, Repeat, Replay, Ticks};
use gilrs::{Axis, Button, EventType, Gilrs};

#[derive(Default, Clone, Copy)]
pub struct Pressed { pub button: Option<Button>, pub any: bool, pub start: bool, pub select: bool, pub up: bool, pub down: bool, pub left: bool, pub right: bool, pub ok: bool, pub back: bool }

/// Buttons that can be given to an action (Start, Select and the d-pad stay reserved).
pub const BINDABLE: [Button; 10] = [Button::South, Button::East, Button::West, Button::North, Button::LeftTrigger, Button::LeftTrigger2,
    Button::RightTrigger, Button::RightTrigger2, Button::LeftThumb, Button::RightThumb];

/// The actions of the controller map, in order: jump, fire, open door, teleport.
pub const ACTIONS: [&str; 4] = ["Jump", "Fire", "Open door", "Teleport"];
pub const DEFAULT_MAP: [Button; 4] = [Button::South, Button::West, Button::East, Button::North];

/// Short names as printed on an Xbox-style pad (also what comic.cfg stores).
pub fn button_name(b: Button) -> &'static str {
    match b {
        Button::South => "A", Button::East => "B", Button::West => "X", Button::North => "Y",
        Button::LeftTrigger => "LB", Button::LeftTrigger2 => "LT", Button::RightTrigger => "RB", Button::RightTrigger2 => "RT",
        Button::LeftThumb => "L3", Button::RightThumb => "R3", _ => "?",
    }
}

pub fn button_from_name(n: &str) -> Option<Button> { BINDABLE.iter().copied().find(|b| button_name(*b) == n) }

pub struct Pad { g: Option<Gilrs>, effect: Option<(Effect, std::time::Instant)> }

impl Pad {
    pub fn new() -> Pad { Pad { g: Gilrs::new().ok(), effect: None } }

    /// Drain the events since the last call: which buttons went down.
    pub fn poll(&mut self) -> Pressed {
        let mut p = Pressed::default();
        // belt and braces: stop the rumble once its time is up
        if self.effect.as_ref().map_or(false, |(_, until)| std::time::Instant::now() >= *until) {
            if let Some((e, _)) = self.effect.take() { let _ = e.stop(); }
        }
        let Some(g) = self.g.as_mut() else { return p };
        while let Some(ev) = g.next_event() {
            if let EventType::ButtonPressed(b, _) = ev.event {
                p.any = true;
                if BINDABLE.contains(&b) && p.button.is_none() { p.button = Some(b); }
                match b {
                    Button::Start => p.start = true,
                    Button::Select => p.select = true,
                    Button::DPadUp => p.up = true,
                    Button::DPadDown => p.down = true,
                    Button::DPadLeft => p.left = true,
                    Button::DPadRight => p.right = true,
                    Button::South => p.ok = true,
                    Button::East => p.back = true,
                    _ => {}
                }
            }
        }
        p
    }

    /// Is any controller connected?
    pub fn connected(&self) -> bool { self.g.as_ref().map_or(false, |g| g.gamepads().next().is_some()) }

    /// Rumble every connected pad that supports it: `strong` is the magnitude (0..=65535), `ms` the duration.
    pub fn rumble(&mut self, strong: u16, ms: u32) {
        let Some(g) = self.g.as_mut() else { return };
        let ids: Vec<_> = g.gamepads().filter(|(_, p)| p.is_ff_supported()).map(|(id, _)| id).collect();
        if ids.is_empty() { return; }
        let play = || Replay { after: Ticks::from_ms(0), play_for: Ticks::from_ms(ms), with_delay: Ticks::from_ms(0) };
        let built = EffectBuilder::new()
            .add_effect(BaseEffect { kind: BaseEffectType::Strong { magnitude: strong }, scheduling: play(), envelope: Default::default() })
            .add_effect(BaseEffect { kind: BaseEffectType::Weak { magnitude: strong / 2 }, scheduling: play(), envelope: Default::default() })
            .gamepads(&ids)
            .repeat(Repeat::For(Ticks::from_ms(ms))) // the default repeats forever
            .finish(g);
        if let Ok(e) = built { let _ = e.play(); self.effect = Some((e, std::time::Instant::now() + std::time::Duration::from_millis(ms as u64 + 100))); }
    }

    /// The held state of the six game actions.
    pub fn held(&self, map: &[Button; 4]) -> Input {
        let mut i = Input::default();
        let Some(g) = self.g.as_ref() else { return i };
        for (_, pad) in g.gamepads() {
            let x = pad.value(Axis::LeftStickX);
            i.left |= pad.is_pressed(Button::DPadLeft) || x < -0.5;
            i.right |= pad.is_pressed(Button::DPadRight) || x > 0.5;
            i.jump |= pad.is_pressed(map[0]);
            i.fire |= pad.is_pressed(map[1]);
            i.open |= pad.is_pressed(map[2]) || pad.is_pressed(Button::DPadDown);
            i.teleport |= pad.is_pressed(map[3]);
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_names_round_trip() {
        for b in BINDABLE { assert_eq!(button_from_name(button_name(b)), Some(b)); }
        assert_eq!(button_from_name("Start"), None);
    }
}
