//! Optional "modern effects": particles, glow, a soft shadow, hit flash / shake, fades between stages and a score that
//! counts up. All of it is drawn on top of the original graphics and driven by what the game does; none of it feeds
//! back into the game.
use crate::{game::{Game, Map, Phase}, Frame, Snap};
use macroquad::prelude::*;

const MAX_PARTICLES: usize = 500;

pub struct Particle { x: f32, y: f32, vx: f32, vy: f32, life: f32, max: f32, size: f32, color: Color, gravity: f32, big: f32 }

/// Per tile: its black pixels (window sky) and its lit console pixels with their colours, for the base's effects.
#[derive(Default, Clone)]
pub struct Spots { pub black: Vec<(u8, u8)>, pub lights: Vec<(u8, u8, [f32; 3])> }

pub struct Fx {
    particles: Vec<Particle>,
    pub flash: f32,
    /// Smooth motion draws a new fireball one step behind its game position; the muzzle flash follows.
    pub smooth: bool,
    pub shake: f32,
    pub shake_off: (f32, f32),
    pub fade: f32,
    pub shown_score: f32,
    pub time: f32,
    /// Seconds since each enemy (by index) appeared; they fade in instead of popping in.
    spawn_age: Vec<f32>,
    glow: Texture2D,
    /// black with a soft clear centre (castle darkness) and with a sharp clear disc (the iris)
    hole: Texture2D,
    iris: Texture2D,
    pub weather: crate::settings::Weather,
    rng: u32,
}

fn glow_texture() -> Texture2D {
    const N: usize = 32;
    let mut px = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = ((x as f32 + 0.5) / N as f32 * 2.0 - 1.0, (y as f32 + 0.5) / N as f32 * 2.0 - 1.0);
            let d = (dx * dx + dy * dy).sqrt().min(1.0);
            let a = (1.0 - d).powi(2);
            px.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    let t = Texture2D::from_rgba8(N as u16, N as u16, &px);
    t.set_filter(FilterMode::Linear);
    t
}

/// A square, white-free texture that is transparent in a disc in the middle and opaque black outside it: the alpha ramps
/// from 0 at radius `inner` to 1 at radius `outer` (both as a fraction of the half width).
fn hole_texture(inner: f32, outer: f32) -> Texture2D {
    const N: usize = 64;
    let mut px = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = ((x as f32 + 0.5) / N as f32 * 2.0 - 1.0, (y as f32 + 0.5) / N as f32 * 2.0 - 1.0);
            let d = (dx * dx + dy * dy).sqrt();
            let t = ((d - inner) / (outer - inner)).clamp(0.0, 1.0);
            px.extend_from_slice(&[0, 0, 0, (t * t * (3.0 - 2.0 * t) * 255.0) as u8]);
        }
    }
    let t = Texture2D::from_rgba8(N as u16, N as u16, &px);
    t.set_filter(FilterMode::Linear);
    t
}

impl Fx {
    pub fn new() -> Fx {
        Fx { particles: vec![], flash: 0.0, smooth: false, shake: 0.0, shake_off: (0.0, 0.0), fade: 0.0, shown_score: 0.0, time: 0.0, spawn_age: vec![], glow: glow_texture(), hole: hole_texture(0.30, 1.0), iris: hole_texture(0.97, 1.0), weather: crate::settings::Weather::Off, rng: 0x2545f491 }
    }

    pub fn reset(&mut self, score: u32) {
        self.particles.clear();
        self.flash = 0.0;
        self.shake = 0.0;
        self.fade = 1.0;
        self.shown_score = score as f32;
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A burst of `n` particles from a world position (pixels), with colours picked from `colors`.
    pub fn burst(&mut self, x: f32, y: f32, n: usize, speed: f32, colors: &[Color], gravity: f32) {
        for _ in 0..n {
            if self.particles.len() >= MAX_PARTICLES { return; }
            let a = self.rand() * std::f32::consts::TAU;
            let s = speed * (0.35 + 0.65 * self.rand());
            let color = colors[(self.rand() * colors.len() as f32) as usize % colors.len()];
            let max = 0.35 + self.rand() * 0.4;
            let size = if self.rand() < 0.3 { 2.0 } else { 1.0 };
            self.particles.push(Particle { x, y, vx: a.cos() * s, vy: a.sin() * s - speed * 0.3, life: max, max, size, color, gravity, big: 0.0 });
        }
    }

    /// A soft flash of light that fades out (muzzle flash, impacts).
    fn flash_at(&mut self, x: f32, y: f32, radius: f32, life: f32, color: Color) {
        self.particles.push(Particle { x, y, vx: 0.0, vy: 0.0, life, max: life, size: 1.0, color, gravity: 0.0, big: radius });
    }

    /// Compare the state before and after a game tick and spawn the matching effects.
    pub fn observe(&mut self, prev: &Snap, g: &Game) {
        let (px, py) = ((g.player.x * 8 + 8) as f32, (g.player.y * 8 + 16) as f32); // middle of Comic
        if self.spawn_age.len() != g.enemies.len() { self.spawn_age = vec![9.0; g.enemies.len()]; }
        if prev.level != g.level_idx || prev.stage != g.stage_idx || prev.resets != g.resets { self.fade = 1.0; self.spawn_age.fill(9.0); }
        let same_stage = prev.level == g.level_idx && prev.stage == g.stage_idx && prev.resets == g.resets;
        // a door opens: a burst of golden light at the doorway
        if let Phase::Door(d) = g.phase {
            if !prev.door {
                const GOLD: [Color; 3] = [Color::new(1.0, 0.9, 0.4, 1.0), Color::new(1.0, 1.0, 0.8, 1.0), Color::new(1.0, 0.7, 0.2, 1.0)];
                let (dx, dy) = ((d.door.1 * 8 + 16) as f32, (d.door.0 * 8 + 16) as f32);
                self.burst(dx, dy, 26, 45.0, &GOLD, -15.0);
                self.flash_at(dx, dy, 30.0, 0.35, Color::new(1.0, 0.9, 0.5, 1.0));
            }
        }
        if !same_stage || g.phase != Phase::Playing { return; }
        const SPARK: [Color; 3] = [Color::new(1.0, 0.9, 0.3, 1.0), Color::new(1.0, 0.55, 0.15, 1.0), Color::new(1.0, 1.0, 0.8, 1.0)];
        const DUST: [Color; 2] = [Color::new(0.75, 0.75, 0.7, 1.0), Color::new(0.55, 0.45, 0.3, 1.0)];
        const RED: [Color; 2] = [Color::new(1.0, 0.3, 0.3, 1.0), Color::new(1.0, 0.7, 0.3, 1.0)];
        const PUFF: [Color; 3] = [Color::new(0.8, 0.9, 1.0, 1.0), Color::new(0.7, 0.5, 1.0, 1.0), Color::new(1.0, 1.0, 1.0, 1.0)];
        // fireballs: a small muzzle flash, and a flash plus sparks where one ends
        let mut killed_at: Vec<(f32, f32)> = vec![];
        for (i, e) in g.enemies.iter().enumerate() {
            if prev.states.get(i) == Some(&1) && e.state >= 2 { killed_at.push((e.x as f32, e.y as f32)); }
        }
        const EMBER: [Color; 3] = [Color::new(1.0, 0.8, 0.2, 1.0), Color::new(1.0, 0.45, 0.1, 1.0), Color::new(1.0, 0.95, 0.6, 1.0)];
        for i in 0..5 {
            match (prev.fbs[i], g.fireballs[i]) {
                (None, Some(f)) => {
                    let back = if self.smooth { f.dx } else { 0 };
                    let (x, y) = (((f.x - back) * 8 + 8) as f32, (f.y * 8 + 4) as f32);
                    self.flash_at(x, y, 12.0, 0.12, Color::new(1.0, 0.9, 0.5, 1.0));
                    self.burst(x, y, 5, 30.0, &EMBER, 0.0);
                }
                (Some((py0, px0)), None) => {
                    let near_kill = killed_at.iter().any(|(kx, ky)| (kx - px0 as f32).abs() <= 3.0 && (ky - py0 as f32).abs() <= 3.0);
                    let on_screen = (0..=0x14).contains(&(px0 - g.player.cam));
                    let (x, y) = ((px0 * 8 + 8) as f32, (py0 * 8 + 4) as f32);
                    if near_kill { self.flash_at(x, y, 18.0, 0.2, Color::new(1.0, 0.85, 0.4, 1.0)); }
                    else if on_screen { self.flash_at(x, y, 12.0, 0.16, Color::new(1.0, 0.6, 0.2, 1.0)); self.burst(x, y, 9, 45.0, &EMBER, 50.0); }
                }
                _ => {}
            }
        }
        for (i, e) in g.enemies.iter().enumerate() {
            if prev.states.get(i) == Some(&0) && e.state == 1 {
                self.spawn_age[i] = 0.0;
                self.burst((e.x * 8 + 8) as f32, (e.y * 8 + 8) as f32, 14, 26.0, &PUFF, 0.0);
            }
            if prev.states.get(i) == Some(&1) && e.state >= 2 {
                self.burst((e.x * 8 + 8) as f32, (e.y * 8 + 8) as f32, 22, 70.0, &SPARK, 90.0);
            }
        }
        if prev.air && !g.player.air { self.burst(px, py + 16.0, 6, 28.0, &DUST, 0.0); }
        if g.hp < prev.hp { self.flash = 1.0; self.shake = 1.0; self.burst(px, py, 16, 60.0, &RED, 60.0); }
        if matches!(g.phase, Phase::Fall(_)) && !prev.fell { self.flash = 1.0; self.shake = 1.0; } // falling out of the bottom of the screen
        if g.dying.is_some() && !prev.dying { self.flash = 1.0; self.shake = 1.4; self.burst(px, py, 40, 90.0, &RED, 80.0); }
        if g.collected.len() > prev.collected { self.burst(px, py - 4.0, 28, 60.0, &SPARK, -20.0); }
    }

    pub fn update(&mut self, dt: f32, g: &Game) {
        let dt = dt.min(0.1);
        self.time += dt;
        for a in &mut self.spawn_age { *a += dt; }
        for p in &mut self.particles { p.x += p.vx * dt; p.y += p.vy * dt; p.vy += p.gravity * dt; p.life -= dt; }
        self.particles.retain(|p| p.life > 0.0);
        self.flash = (self.flash - dt * 5.0).max(0.0);
        self.shake = (self.shake - dt * 4.0).max(0.0);
        self.fade = (self.fade - dt * 3.3).max(0.0);
        self.shake_off = if self.shake > 0.0 { ((self.rand() - 0.5) * 4.0 * self.shake, (self.rand() - 0.5) * 4.0 * self.shake) } else { (0.0, 0.0) };
        // a few sparks fall off each fireball
        if g.phase == Phase::Playing {
            for f in g.fireballs.iter().flatten() {
                if self.rand() < dt * 14.0 && self.particles.len() < MAX_PARTICLES {
                    let (x, y) = ((f.x * 8 + 8) as f32 - f.dx as f32 * 6.0, (f.y * 8 + 4) as f32);
                    let (vx, vy) = (-(f.dx as f32) * 4.0, (self.rand() - 0.5) * 20.0);
                    let c = if self.rand() < 0.5 { Color::new(1.0, 0.8, 0.2, 1.0) } else { Color::new(1.0, 0.45, 0.1, 1.0) };
                    self.particles.push(Particle { x, y, vx, vy, life: 0.18, max: 0.18, size: 1.0, color: c, gravity: 0.0, big: 0.0 });
                }
            }
        }
        // the score counts up
        let target = g.score as f32;
        if target < self.shown_score { self.shown_score = target; }
        else { self.shown_score += (target - self.shown_score) * (1.0 - (-dt * 7.0).exp()); if target - self.shown_score < 0.5 { self.shown_score = target; } }
    }

    /// How visible enemy `i` is: 0 right after it appears, 1 after a moment.
    pub fn spawn_alpha(&self, i: usize) -> f32 { (self.spawn_age.get(i).copied().unwrap_or(9.0) / 0.45).clamp(0.0, 1.0) }

    /// A soft coloured glow centred on a virtual-screen position.
    pub fn glow(&self, f: &Frame, cx: f32, cy: f32, r: f32, color: Color) {
        let (x, y, w, h) = f.px(cx - r, cy - r, 2.0 * r, 2.0 * r);
        draw_texture_ex(&self.glow, x, y, color, DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
    }

    /// A soft shadow on the ground, centred on a virtual-screen position.
    pub fn shadow(&self, f: &Frame, cx: f32, cy: f32) {
        draw_ellipse(f.x + cx * f.s, f.y + cy * f.sy, 7.0 * f.s, 1.7 * f.sy, 0.0, Color::new(0.0, 0.0, 0.0, 0.35));
    }

    /// Level atmosphere, drawn over the tiles and under the sprites. Lake (level 0): glints on the
    /// water; space (level 2): twinkling stars.
    pub fn draw_ambient(&self, f: &Frame, level: usize, map: &Map, cam: f32, spots: Option<&[Spots]>) {
        let hash = |a: u32, b: u32| -> u32 {
            let mut v = a.wrapping_mul(0x9e3779b1) ^ b.wrapping_mul(0x85ebca6b) ^ 0x27d4eb2f;
            v ^= v >> 15; v = v.wrapping_mul(0x2c1b3c6d); v ^= v >> 12; v = v.wrapping_mul(0x297a2d39); v ^ (v >> 15)
        };
        let t = self.time;
        let cam_px = cam * 8.0;
        let first = (cam_px / 16.0).floor().max(0.0) as usize;
        let count = ((192.0 + f.ex) / 16.0).ceil() as usize + 2;
        match level {
            0 => {
                // glints on the plain water tile (46) and the water half of the island tiles (33..=35)
                for c in first..(first + count).min(map.w) {
                    for r in 0..map.h {
                        let id = map.cells[r * map.w + c];
                        let (top, rows) = match id { 46 => (0.0, 16u32), 33..=35 => (8.0, 8u32), _ => continue };
                        for g in 0..2u32 {
                            let seed = hash(c as u32 * 64 + r as u32, g);
                            let phase = (seed % 628) as f32 / 100.0;
                            let a = ((t * 1.1 + phase).sin()).max(0.0).powi(3) * 0.55;
                            if a < 0.05 { continue; }
                            let (gx, gy) = ((seed >> 8) % 11, (seed >> 16) % rows);
                            let w = 3.0 + ((seed >> 4) % 3) as f32;
                            let (x, y, ww, hh) = f.px(8.0 + c as f32 * 16.0 - cam_px + gx as f32, 8.0 + r as f32 * 16.0 + top + gy as f32, w, 1.0);
                            draw_rectangle(x, y, ww.max(1.0), hh.max(1.0), Color::new(0.8, 1.0, 1.0, a));
                        }
                    }
                }
            }
            2 => {
                // stars: in the tiles that hold stars, one of them twinkles
                for c in first..(first + count).min(map.w) {
                    for r in 0..map.h {
                        let id = map.cells[r * map.w + c];
                        if !(matches!(id, 0 | 1 | 2 | 4) || (56..=71).contains(&id)) { continue; }
                        let seed = hash(c as u32 * 64 + r as u32, 7);
                        let phase = (seed % 628) as f32 / 100.0;
                        let a = 0.5 + 0.5 * (t * (1.4 + (seed >> 12) as f32 % 3.0 * 0.6) + phase).sin();
                        if a < 0.35 { continue; }
                        let (sx, sy) = (2.0 + ((seed >> 8) % 12) as f32, 2.0 + ((seed >> 16) % 12) as f32);
                        let (bx, by) = (8.0 + c as f32 * 16.0 - cam_px + sx, 8.0 + r as f32 * 16.0 + sy);
                        let col = Color::new(0.85, 0.92, 1.0, a);
                        let (x, y, w, h) = f.px(bx, by, 1.0, 1.0);
                        draw_rectangle(x, y, w.max(1.0), h.max(1.0), col);
                        if a > 0.85 {
                            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                                let (x, y, w, h) = f.px(bx + dx, by + dy, 1.0, 1.0);
                                draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(0.85, 0.92, 1.0, a * 0.5));
                            }
                        }
                    }
                }
            }
            1 => {
                // fireflies drift about in the dark spaces between the trees and blink
                for c in first..(first + count).min(map.w) {
                    for r in 3..map.h.min(9) {
                        if map.cells[r * map.w + c] != 0 { continue; }
                        let seed = hash(c as u32 * 64 + r as u32, 3);
                        if seed % 7 != 0 { continue; }
                        let (p1, p2, p3) = ((seed % 628) as f32 / 100.0, ((seed >> 9) % 628) as f32 / 100.0, ((seed >> 18) % 628) as f32 / 100.0);
                        let (cx, cy) = (c as f32 * 16.0 + 8.0 + (t * 0.6 + p1).sin() * 9.0, r as f32 * 16.0 + 8.0 + (t * 0.8 + p2).cos() * 7.0);
                        let blink = (t * 1.6 + p3).sin().max(0.0).powi(2);
                        if blink < 0.05 { continue; }
                        let (vx, vy) = (8.0 + cx - cam_px, 8.0 + cy);
                        self.glow(f, vx, vy, 6.0, Color::new(0.75, 1.0, 0.3, 0.8 * blink));
                        let (x, y, w, h) = f.px(vx - 0.5, vy - 0.5, 1.0, 1.0);
                        draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(1.0, 1.0, 0.75, blink));
                    }
                }
            }
            6 => {
                // flickering torchlight (60 = brazier, 61 = wall torch) with a few rising embers
                for c in first..(first + count).min(map.w) {
                    for r in 0..map.h {
                        let id = map.cells[r * map.w + c];
                        if id != 60 && id != 61 { continue; }
                        let seed = hash(c as u32 * 64 + r as u32, 11);
                        let p = (seed % 628) as f32 / 100.0;
                        let flick = 0.8 + 0.12 * (t * 9.0 + p).sin() + 0.08 * (t * 23.0 + p * 2.0).sin();
                        let big = if id == 60 { 1.3 } else { 1.0 };
                        let (cx, cy) = (8.0 + c as f32 * 16.0 - cam_px + 8.0, 8.0 + r as f32 * 16.0 + if id == 60 { 6.0 } else { 4.0 });
                        self.torch_light(f, cx, cy, big, flick, p);
                    }
                }
            }
            4 => {
                // the cave: wall torches (tile 6), sparkling damp walls (tiles 1..=5), and water dripping from the ceiling
                for c in first..(first + count).min(map.w) {
                    for r in 0..map.h {
                        let id = map.cells[r * map.w + c];
                        let seed = hash(c as u32 * 64 + r as u32, 5);
                        let p = (seed % 628) as f32 / 100.0;
                        if id == 6 {
                            let flick = 0.8 + 0.12 * (t * 9.0 + p).sin() + 0.08 * (t * 23.0 + p * 2.0).sin();
                            self.torch_light(f, 8.0 + c as f32 * 16.0 - cam_px + 8.0, 8.0 + r as f32 * 16.0 + 5.0, 1.0, flick, p);
                        } else if (1..=5).contains(&id) && seed % 3 == 0 {
                            let a = (t * 1.2 + p).sin().max(0.0).powi(4);
                            if a > 0.1 {
                                let (x, y, w, h) = f.px(8.0 + c as f32 * 16.0 - cam_px + ((seed >> 8) % 14) as f32, 8.0 + r as f32 * 16.0 + ((seed >> 16) % 14) as f32, 1.0, 1.0);
                                draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(0.6, 1.0, 1.0, a));
                            }
                        } else if id > map.solid_above && r + 1 < map.h && map.cells[(r + 1) * map.w + c] <= map.solid_above && seed % 5 == 0 {
                            // a drip: forms at the bottom of the rock, falls to the next solid tile below and splashes
                            let mut n = 1;
                            while r + n + 1 < map.h && map.cells[(r + n + 1) * map.w + c] <= map.solid_above && n < 6 { n += 1; }
                            let period = 2.6 + (seed >> 5) as f32 % 20.0 * 0.12;
                            let ph = ((t + p) / period).rem_euclid(1.0);
                            let (bx, by) = (8.0 + c as f32 * 16.0 - cam_px + 3.0 + ((seed >> 12) % 10) as f32, 8.0 + (r + 1) as f32 * 16.0);
                            let fall = n as f32 * 16.0 - 2.0;
                            if ph < 0.1 {
                                let (x, y, w, h) = f.px(bx, by + ph * 10.0, 1.0, 1.0 + ph * 10.0);
                                draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(0.6, 0.85, 1.0, 0.9));
                            } else if ph < 0.1 + 0.25 {
                                let q = (ph - 0.1) / 0.25;
                                let (x, y, w, h) = f.px(bx, by + 2.0 + q * q * fall, 1.0, 2.0);
                                draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(0.7, 0.9, 1.0, 0.95));
                            } else if ph < 0.1 + 0.25 + 0.06 {
                                let q = (ph - 0.35) / 0.06;
                                for dx in [-2.0 - q * 2.0, 2.0 + q * 2.0] {
                                    let (x, y, w, h) = f.px(bx + dx, by + fall - 1.0 - q * 2.0, 1.0, 1.0);
                                    draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(0.7, 0.9, 1.0, 1.0 - q));
                                }
                            }
                        }
                    }
                }
            }
            3 => {
                // the space base: console lights blink on the lit pixels of the consoles (tiles 31, 34, 36, 37) and
                // stars twinkle on the black pixels of the windows and viewports
                let Some(spots) = spots else { return };
                for c in first..(first + count).min(map.w) {
                    for r in 0..map.h {
                        let id = map.cells[r * map.w + c] as usize;
                        let Some(sp) = spots.get(id) else { continue };
                        let seed = hash(c as u32 * 64 + r as u32, 9);
                        let (ox, oy) = (8.0 + c as f32 * 16.0 - cam_px, 8.0 + r as f32 * 16.0);
                        if matches!(id, 31 | 34 | 36 | 37) && !sp.lights.is_empty() {
                            for k in 0..3u32 {
                                let s2 = hash(seed, k);
                                let on = ((t * (0.8 + (s2 % 5) as f32 * 0.3) + (s2 % 100) as f32 / 16.0).sin()) > 0.3;
                                if !on { continue; }
                                let (px, py, col) = sp.lights[s2 as usize % sp.lights.len()];
                                let (lx, ly) = (ox + px as f32 + 0.5, oy + py as f32 + 0.5);
                                self.glow(f, lx, ly, 5.0, Color::new(col[0], col[1], col[2], 0.6));
                                let (x, y, w, h) = f.px(lx - 0.5, ly - 0.5, 1.0, 1.0);
                                draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(col[0], col[1], col[2], 1.0));
                            }
                        } else if !sp.black.is_empty() && matches!(id, 16..=21 | 48..=59) {
                            for k in 0..2u32 {
                                let s2 = hash(seed, 20 + k);
                                let a = 0.5 + 0.5 * (t * (1.4 + (s2 >> 12) as f32 % 3.0 * 0.6) + (s2 % 628) as f32 / 100.0).sin();
                                if a < 0.4 { continue; }
                                let (px, py) = sp.black[s2 as usize % sp.black.len()];
                                let (x, y, w, h) = f.px(ox + px as f32, oy + py as f32, 1.0, 1.0);
                                draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(0.85, 0.92, 1.0, a));
                            }
                        }
                    }
                }
            }
            5 => {
                // the shed: glints on the water, a pulse running along the red pipe (tiles 21..=23) and glowing coloured blocks
                for c in first..(first + count).min(map.w) {
                    for r in 0..map.h {
                        let id = map.cells[r * map.w + c];
                        let (ox, oy) = (8.0 + c as f32 * 16.0 - cam_px, 8.0 + r as f32 * 16.0);
                        let seed = hash(c as u32 * 64 + r as u32, 5);
                        match id {
                            4 | 10 | 11 => {
                                let (top, rows) = if id == 4 { (8.0, 8u32) } else { (0.0, 16u32) };
                                for g in 0..2u32 {
                                    let s2 = hash(seed, g);
                                    let a = ((t * 1.1 + (s2 % 628) as f32 / 100.0).sin()).max(0.0).powi(3) * 0.55;
                                    if a < 0.05 { continue; }
                                    let (x, y, w, h) = f.px(ox + ((s2 >> 8) % 11) as f32, oy + top + ((s2 >> 16) % rows) as f32, 3.0 + ((s2 >> 4) % 3) as f32, 1.0);
                                    draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(0.8, 1.0, 1.0, a));
                                }
                            }
                            21..=23 => if let Some(sp) = spots.and_then(|s| s.get(id as usize)) { self.pulse(f, &sp.lights, c as f32 * 16.0, r as f32 * 16.0, ox, oy, 0.14, 3.0); },
                            24 | 28..=31 => {
                                let col = match id { 24 => (0.3, 1.0, 1.0), 28 => (0.3, 1.0, 0.3), 29 => (0.45, 0.55, 1.0), 30 => (1.0, 0.3, 0.3), _ => (1.0, 0.4, 1.0) };
                                let glow = 0.5 + 0.5 * (t * 1.2 + (seed % 628) as f32 / 100.0).sin();
                                self.glow(f, ox + 8.0, oy + 8.0, 17.0, Color::new(col.0, col.1, col.2, 0.16 + 0.14 * glow));
                                self.sweep(f, ox, oy, t * 0.5 + (seed % 100) as f32 / 100.0);
                            }
                            _ => {}
                        }
                    }
                }
            }
            7 => {
                // the computer: data pulses along the circuit lines, glowing discs, rolling lines on the magenta panels
                for c in first..(first + count).min(map.w) {
                    for r in 0..map.h {
                        let id = map.cells[r * map.w + c];
                        let (ox, oy) = (8.0 + c as f32 * 16.0 - cam_px, 8.0 + r as f32 * 16.0);
                        let seed = hash(c as u32 * 64 + r as u32, 7);
                        match id {
                            0 | 1 | 6..=13 => if let Some(sp) = spots.and_then(|s| s.get(id as usize)) { self.pulse(f, &sp.lights, c as f32 * 16.0, r as f32 * 16.0, ox, oy, 0.12, 3.5); },
                            41..=50 => {
                                let col = if id <= 44 { (0.5, 0.45, 1.0) } else { (1.0, 0.3, 0.3) };
                                let glow = 0.5 + 0.5 * (t * 1.5 + (seed % 628) as f32 / 100.0).sin();
                                self.glow(f, ox + 8.0, oy + 8.0, 15.0, Color::new(col.0, col.1, col.2, 0.14 + 0.16 * glow));
                            }
                            2..=5 => {
                                let y = ((t * 9.0 + (seed % 16) as f32) as u32 % 16) as f32;
                                let (x, yy, w, h) = f.px(ox, oy + y, 16.0, 1.0);
                                draw_rectangle(x, yy, w, h.max(1.0), Color::new(1.0, 0.85, 1.0, 0.22));
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Cover the playfield view `(x0, y0, x1, y1)` with black of the given opacity, except a clear disc of radius `r` at
    /// (cx, cy) whose edge is set by the texture.
    fn cover(&self, f: &Frame, cx: f32, cy: f32, r: f32, view: (f32, f32, f32, f32), alpha: f32, tex: &Texture2D) {
        let (x0, y0, x1, y1) = view;
        let (l, t, rr, b) = (cx - r, cy - r, cx + r, cy + r);
        let col = Color::new(0.0, 0.0, 0.0, alpha);
        let rect = |xa: f32, ya: f32, xb: f32, yb: f32| {
            let (xa, ya, xb, yb) = (xa.max(x0), ya.max(y0), xb.min(x1), yb.min(y1));
            if xb > xa && yb > ya { let r = f.px(xa, ya, xb - xa, yb - ya); draw_rectangle(r.0, r.1, r.2, r.3, col); }
        };
        rect(x0, y0, x1, t);
        rect(x0, b, x1, y1);
        rect(x0, t, l, b);
        rect(rr, t, x1, b);
        let (x, y, w, h) = f.px(l, t, 2.0 * r, 2.0 * r);
        draw_texture_ex(tex, x, y, Color::new(1.0, 1.0, 1.0, alpha), DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
    }

    /// The lit castle: dim everywhere except around Comic.
    pub fn darkness(&self, f: &Frame, cx: f32, cy: f32, view: (f32, f32, f32, f32)) {
        self.cover(f, cx, cy, 84.0, view, 0.55, &self.hole);
    }

    /// Arriving at a stage: the picture opens as a circle around Comic while `fade` runs from 1 to 0.
    pub fn iris(&self, f: &Frame, cx: f32, cy: f32, view: (f32, f32, f32, f32)) {
        if self.fade <= 0.0 { return; }
        let k = 1.0 - self.fade;
        self.cover(f, cx, cy, (2.0 + 300.0 * k * k).max(2.0), view, 1.0, &self.iris);
    }

    /// A wide soft ellipse of colour.
    fn glow_ellipse(&self, f: &Frame, cx: f32, cy: f32, rx: f32, ry: f32, color: Color) {
        let (x, y, w, h) = f.px(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry);
        draw_texture_ex(&self.glow, x, y, color, DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
    }

    /// Low drifting mist near the ground in the forest (anchored to the world).
    pub fn draw_mist(&self, f: &Frame, level: usize, map: &Map, cam: f32) {
        if level != 1 { return; }
        let color = Color::new(0.8, 0.95, 0.85, 0.20);
        let t = self.time;
        let cam_px = cam * 8.0;
        let first = (cam_px / 16.0).floor().max(0.0) as usize;
        for k in first..first + ((192.0 + f.ex) / 16.0).ceil() as usize + 3 {
            let seed = (k as u32).wrapping_mul(0x9e3779b1) ^ 0x5bd1e995;
            let p = (seed % 628) as f32 / 100.0;
            let cx = 8.0 + k as f32 * 16.0 - cam_px + (t * 0.5 + p).sin() * 34.0 + (t * 0.28 + p * 1.7).cos() * 14.0;
            let cy = 8.0 + 122.0 + ((seed >> 8) % 28) as f32 + (t * 0.4 + p).cos() * 8.0;
            if k >= map.w { break; }
            let pulse = 0.65 + 0.35 * (t * 0.6 + p * 2.0).sin();
            self.glow_ellipse(f, cx, cy, 38.0 + ((seed >> 16) % 16) as f32, 14.0, Color::new(color.r, color.g, color.b, color.a * pulse));
        }
    }

    /// Rain over the outdoor levels (lake and forest), in screen space.
    pub fn draw_weather(&self, f: &Frame, level: usize) {
        use crate::settings::Weather;
        if self.weather == Weather::Off || level > 1 { return; }
        let (w, h) = (192.0 + f.ex, 160.0);
        let t = self.time;
        let n = 110;
        for i in 0..n {
            let seed = (i as u32).wrapping_mul(0x9e3779b1) ^ 0x7f4a7c15;
            let (r1, r2, r3) = ((seed % 1000) as f32 / 1000.0, ((seed >> 10) % 1000) as f32 / 1000.0, ((seed >> 20) % 1000) as f32 / 1000.0);
            {
                let speed = 150.0 + 60.0 * r3;
                let y = (t * speed + r2 * (h + 20.0)).rem_euclid(h + 20.0) - 10.0;
                let x = (r1 * (w + 40.0) - t * 22.0 * (1.0 + r3)).rem_euclid(w + 40.0) - 20.0;
                let (x0, y0, x1, y1) = (8.0 + x, 8.0 + y, 8.0 + x - 2.0, 8.0 + y + 6.0);
                if y < 0.0 || y > h { continue; }
                draw_line(f.x + x0 * f.s, f.y + y0 * f.sy, f.x + x1 * f.s, f.y + y1 * f.sy, (f.s * 0.5).max(1.0), Color::new(0.75, 0.85, 1.0, 0.45));
            }
        }
    }

    /// A pulse of light running along the lit pixels of a tile: pixels whose position along the diagonal matches a
    /// moving wave brighten (and the strongest ones glow). `wx`, `wy` are the tile's world pixel position (for a wave
    /// that runs across tile borders), `ox`, `oy` its place on the screen.
    fn pulse(&self, f: &Frame, lights: &[(u8, u8, [f32; 3])], wx: f32, wy: f32, ox: f32, oy: f32, freq: f32, speed: f32) {
        let t = self.time;
        for &(px, py, col) in lights {
            let v = (((wx + px as f32) + (wy + py as f32)) * freq - t * speed).sin().max(0.0).powi(8);
            if v < 0.2 { continue; }
            let (x, y, w, h) = f.px(ox + px as f32, oy + py as f32, 1.0, 1.0);
            let k = 0.75 * v;
            draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(col[0] + (1.0 - col[0]) * k, col[1] + (1.0 - col[1]) * k, col[2] + (1.0 - col[2]) * k, 0.4 + 0.6 * v));
            if v > 0.7 { self.glow(f, ox + px as f32 + 0.5, oy + py as f32 + 0.5, 3.0, Color::new(col[0], col[1], col[2], 0.35 * v)); }
        }
    }

    /// A bright diagonal sweeping across one 16x16 tile (`p` runs 0..1 and wraps; it is off most of the time).
    fn sweep(&self, f: &Frame, ox: f32, oy: f32, p: f32) {
        let p = p.fract() * 2.2 - 0.3; // 0..1 crosses the tile
        if !(0.0..=1.0).contains(&p) { return; }
        let d = p * 30.0;
        for x in 0..16 {
            for dy in 0..2 {
                let y = d - x as f32 + dy as f32;
                if (0.0..16.0).contains(&y) {
                    let (sx, sy, w, h) = f.px(ox + x as f32, oy + y.floor(), 1.0, 1.0);
                    draw_rectangle(sx, sy, w.max(1.0), h.max(1.0), Color::new(1.0, 1.0, 1.0, 0.35));
                }
            }
        }
    }

    /// Flickering warm light around a torch or brazier at a virtual-screen position, with two rising embers.
    fn torch_light(&self, f: &Frame, cx: f32, cy: f32, big: f32, flick: f32, p: f32) {
        let t = self.time;
        self.glow(f, cx, cy, 42.0 * big * flick, Color::new(1.0, 0.55, 0.2, 0.16 * flick));
        self.glow(f, cx, cy, 20.0 * big * flick, Color::new(1.0, 0.7, 0.3, 0.38 * flick));
        for k in 0..2u32 {
            let ph = (t * 0.8 + p + k as f32 * 0.5).rem_euclid(1.0);
            let (x, y, w, h) = f.px(cx + (ph * 7.0 + p + k as f32 * 2.0).sin() * 3.0, cy - 2.0 - ph * 16.0, 1.0, 1.0);
            draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(1.0, 0.7, 0.25, (1.0 - ph) * 0.8));
        }
    }

    /// Particles (world pixel coordinates) for a camera at `cam` units.
    pub fn draw_particles(&self, f: &Frame, cam: f32) {
        for p in &self.particles {
            let a = (p.life / p.max).clamp(0.0, 1.0);
            if p.big > 0.0 {
                self.glow(f, 8.0 + p.x - cam * 8.0, 8.0 + p.y, p.big * (1.3 - 0.3 * a), Color::new(p.color.r, p.color.g, p.color.b, a * 0.8));
                continue;
            }
            let (x, y, w, h) = f.px(8.0 + p.x - cam * 8.0, 8.0 + p.y, p.size, p.size);
            draw_rectangle(x, y, w.max(1.0), h.max(1.0), Color::new(p.color.r, p.color.g, p.color.b, a));
        }
    }
}
