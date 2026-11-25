//! Faithful port of the original game rules (see NOTES.md). All positions are in 8-pixel units.
use crate::data::{Level, Stage};
use crate::sound::Sound;
use std::collections::HashMap;

/// Play a sound through the game's model of the PC speaker (see `Game::play_fields`).
macro_rules! snd {
    ($s:ident, $x:expr) => { Game::play_fields(&mut $s.speaker, &$s.sound_len, &mut $s.sounds, $x) };
}

/// The original ran one logic step every 2 BIOS timer ticks (18.2065 Hz / 2).
pub const TICK_SECS: f32 = 2.0 * 65536.0 / 1_193_182.0;

#[derive(Default, Clone, Copy)]
pub struct Input { pub left: bool, pub right: bool, pub jump: bool, pub open: bool, pub fire: bool, pub teleport: bool }

/// Tile map of the current stage plus the tileset's solidity threshold (tile id > threshold is solid).
pub struct Map { pub cells: Vec<u8>, pub w: usize, pub h: usize, pub solid_above: u8 }

impl Map {
    pub fn solid(&self, y: i32, x: i32) -> bool {
        if y < 0 || x < 0 { return false; }
        let (r, c) = ((y >> 1) as usize, (x >> 1) as usize);
        r < self.h && c < self.w && (self.cells[r * self.w + c] as i8) > (self.solid_above as i8)
    }
    /// Solid check for a 2-unit-wide body whose left edge is `x` (checks the neighbour when straddling).
    fn solid2(&self, y: i32, x: i32) -> bool { self.solid(y, x) || (x & 1 == 1 && self.solid(y, x + 1)) }
}

#[derive(Default, Clone)]
pub struct Items { pub corkscrew: bool, pub key: bool, pub boots: bool, pub lantern: bool, pub wand: bool }

/// Result of a tick that the host must react to.
pub enum Event { None, ChangeStage { stage: usize, y: i32, x: i32 }, EnterDoor { level: usize, stage: usize, y: i32, x: i32 }, Fell, Teleport }

pub struct Player {
    pub y: i32, pub x: i32, pub cam: i32,
    pub vx: i32, pub vy: i32, pub air: bool,
    jump_ctr: i32, carry: i32, walk: i32,
    pub facing: i32, // 0 right, 5 left
    pub anim: i32,   // 0 stand, 1..3 walk, 4 jump
}

impl Player {
    pub fn new(y: i32, x: i32) -> Player {
        let mut p = Player { y, x, cam: 0, vx: 0, vy: 0, air: false, jump_ctr: 4, carry: 0, walk: 0, facing: 0, anim: 0 };
        p.snap_camera();
        p
    }
    /// Reload the jump counter (the original does this in its idle loop whenever Comic stands and jump is not held).
    pub fn reload_jump(&mut self, v: i32) { self.jump_ctr = v; }
    /// Per-tick bookkeeping that the original does before anything else (walk cycle 1..3, animation reset).
    pub fn begin_tick(&mut self) {
        self.walk = if self.walk >= 3 { 1 } else { self.walk + 1 };
        self.anim = 0;
    }
    #[allow(dead_code)] // debugging accessors (used by the trace tooling)
    pub fn walk_phase(&self) -> i32 { self.walk }
    #[allow(dead_code)]
    pub fn jump_counter(&self) -> i32 { self.jump_ctr }
    /// Move to a new stage position but keep all motion state (the original only resets it on death).
    /// Original 0x1688: leaving a stage through its left/right edge zeroes the vertical velocity.
    pub fn stop_vertical(&mut self) { self.vy = 0; }
    pub fn relocate(&mut self, y: i32, x: i32) { self.y = y; self.x = x; self.snap_camera(); }
    pub fn snap_camera(&mut self) { self.cam = (self.x - 11).clamp(0, 232); }
    pub fn frame(&self) -> usize { (self.facing + self.anim) as usize }

    fn step_left(&mut self, map: &Map, left_exit: Option<u8>) -> Event {
        if self.x == 0 {
            return match left_exit {
                Some(s) => Event::ChangeStage { stage: s as usize, y: self.y, x: 0xfe },
                None => { self.vx = 0; Event::None }
            };
        }
        if map.solid(self.y + 3, self.x - 1) { self.vx = 0; return Event::None; }
        self.x -= 1;
        self.anim = self.walk;
        if self.cam != 0 && self.x - self.cam < 10 { self.cam -= 1; }
        Event::None
    }

    fn step_right(&mut self, map: &Map, right_exit: Option<u8>) -> Event {
        if self.x == 0xfe {
            return match right_exit {
                Some(s) => Event::ChangeStage { stage: s as usize, y: self.y, x: 0 },
                None => { self.vx = 0; Event::None }
            };
        }
        if map.solid(self.y + 3, self.x + 2) { self.vx = 0; return Event::None; }
        self.x += 1;
        self.anim = self.walk;
        if self.cam < 0xe8 && self.x - self.cam > 12 { self.cam += 1; }
        Event::None
    }

    /// One original game tick.
    pub fn tick(&mut self, inp: Input, map: &Map, stage: &crate::data::Stage, level_idx: usize, items: &Items) -> Event {
        let boots = if items.boots { 5 } else { 4 };

        if !self.air {
            if inp.jump {
                if self.jump_ctr != 1 { self.air = true; }
            } else {
                self.jump_ctr = boots;
            }
        }
        if !self.air {
            if inp.open && items.key {
                for d in &stage.doors {
                    let diff = (self.x - d.x as i32) as u8;
                    // original: NEG DH; ADD DH,AH  => px - door.x (unsigned byte) < 3
                    if self.y == d.y as i32 && diff < 3 {
                        return Event::EnterDoor { level: d.level as usize, stage: d.stage as usize, y: d.y as i32, x: d.x as i32 };
                    }
                }
            }
            if inp.teleport && items.wand { return Event::Teleport; }
            self.vx = 0;
            if inp.left {
                self.vx = -5;
                if self.facing != 5 { self.facing = 5; } else if let e @ Event::ChangeStage { .. } = self.step_left(map, stage.left) { return e; }
            }
            if inp.right {
                self.vx = 5;
                if self.facing != 0 { self.facing = 0; } else if let e @ Event::ChangeStage { .. } = self.step_right(map, stage.right) { return e; }
            }
            if !map.solid2(self.y + 4, self.x) {
                // walked off a ledge
                self.vy = 8;
                self.vx = self.vx.signum() * 2;
                self.air = true;
                self.jump_ctr = 1;
            }
            return Event::None;
        }
        self.air_tick(inp, map, stage, level_idx, boots)
    }

    fn air_tick(&mut self, inp: Input, map: &Map, stage: &crate::data::Stage, level_idx: usize, _boots: i32) -> Event {
        self.jump_ctr -= 1;
        if self.jump_ctr == 0 {
            self.jump_ctr = 1;
            self.carry = 0;
        } else if inp.jump {
            self.vy -= 7;
        } else {
            self.carry = 0;
        }
        let mut ny = self.y + (self.vy >> 3);
        if ny < 0 { ny = 0; }
        ny += self.carry;
        self.carry = 0;
        self.y = ny;
        if ny >= 0x11 { return Event::Fell; }

        let mut g = self.vy;
        if level_idx == 2 { g -= 2; }
        g += 5;
        self.vy = g.min(0x17);

        if inp.left { self.facing = 5; self.vx = (self.vx - 1).max(-5); }
        if inp.right { self.facing = 0; self.vx = (self.vx + 1).min(5); }
        if self.vx < 0 {
            self.vx += 1;
            if let e @ Event::ChangeStage { .. } = self.step_left(map, stage.left) { return e; }
        }
        if self.vx > 0 {
            self.vx -= 1;
            if let e @ Event::ChangeStage { .. } = self.step_right(map, stage.right) { return e; }
        }

        let head = map.solid2(self.y, self.x);
        if head && self.vy <= 0 {
            self.carry = 1;
            self.vy = 0;
            self.anim = 4;
        } else if self.vy > 0 {
            if map.solid2(self.y + 5, self.x) {
                if self.y < 0xf {
                    self.y = (self.y + 1) & !1;
                    self.air = false;
                    self.vy = 0;
                } else {
                    self.anim = 4;
                }
            } else {
                self.anim = 4;
            }
        } else {
            self.anim = 4;
        }
        Event::None
    }
}


/// Item pickup test from the original (|dx| < 2, 0 <= dy < 4).
pub fn touches_item(p: &Player, item_y: u8, item_x: u8) -> bool {
    let dx = (item_x as i32 - p.x).abs();
    let dy = item_y as i32 - p.y;
    dx < 2 && (0..4).contains(&dy)
}

// ---------------------------------------------------------------------------------------------
// Enemies, fireballs, items, health: a port of the original's per-tick logic (see NOTES.md).
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Enemy {
    pub y: i32, pub x: i32,
    pub dx: i32,      // horizontal direction (1, -1, 0)
    pub vy: i32,      // vertical velocity / direction (meaning depends on behaviour)
    pub frame: usize, // animation frame
    pub count: usize, // number of animation frames
    pub beh: u8,      // behaviour 1..=5 (0x7f = disabled)
    pub state: i32,   // 0 inactive (timer counts down), 1 alive, 2..=13 exploding, 7 = despawn next tick
    pub face: usize,  // frame-set offset: 0 or 5 (moving right)
    pub toggle: i32,  // speed toggle (starts at 2 for "fast" enemies)
    pub timer: i32,   // respawn countdown while inactive
    pub sprite: usize,// index into the level's sprite list
}

#[derive(Clone, Copy, Debug)]
pub struct Fireball { pub y: i32, pub x: i32, pub dx: i32, pub ctr: i32, pub frame: usize }

#[derive(Clone, Copy, Debug)]
pub struct Teleport { pub step: u8, pub orig: (i32, i32), pub dest: (i32, i32), pub scroll: i32, pub scroll_dir: i32 }

/// Which part of the game is running (opening beam-in, play, ending sequence, game over).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase { Opening(u32), Playing, Door(DoorAnim), Fall(u32), Ending(Ending), GameOver }

/// Drawing steps of the door animation (original 0x0d25 / 0x0ca8), executed in order on the playfield.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DoorOp { Playfield, Clear, Edges, Player }

use DoorOp::*;
const DEPART: [&[DoorOp]; 4] = [
    &[Playfield, Clear, Edges, Player],
    &[Playfield, Clear, Player],
    &[Playfield, Edges, Clear, Player, Edges],
    &[Playfield],
];
const ARRIVE: [&[DoorOp]; 5] = [
    &[Playfield],
    &[Playfield, Clear, Player, Edges],
    &[Playfield, Clear, Player],
    &[Playfield, Edges, Clear, Edges, Player],
    &[Playfield, Player],
];

/// Leaving through a door: 4 ticks. Arriving: 3 ticks of waiting on the old scene, then 5 frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoorAnim {
    pub arriving: bool,
    pub frame: u32,
    pub level: usize, pub stage: usize, pub cam: i32, // the scene being shown
    pub old: (usize, usize, i32),                      // the scene shown while waiting to arrive
    pub door: (i32, i32),                              // (y, x) of the 4x4-unit door area
    pending: Option<(usize, usize, i32, i32)>,         // destination (level, stage, y, x), applied when leaving ends
}

impl DoorAnim {
    /// (level, stage, camera) of the map to draw now.
    pub fn scene(&self) -> (usize, usize, i32) {
        if self.arriving && self.frame < 3 { self.old } else { (self.level, self.stage, self.cam) }
    }
    pub fn ops(&self) -> &'static [DoorOp] {
        if !self.arriving { DEPART[self.frame.min(3) as usize] }
        else if self.frame < 3 { &[Playfield] }
        else { ARRIVE[(self.frame - 3).min(4) as usize] }
    }
}

/// Steps of the victory sequence (original 0x109d).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ending {
    BeamOut(u32),        // 12 beam frames + 6 empty ticks
    Bonus(u32),          // 20 ticks, +10 each
    LifeBonus(u32),      // per spare life: 10 ticks +10 each, then 3 ticks
    Screen(u32),         // story text screen; waits for a key
    Banner,              // "Game Over" banner; waits for a key
}

pub struct Game {
    pub phase: Phase,
    pub sounds: Vec<Sound>, // sounds the speaker accepted, for the host to play
    sound_len: HashMap<Sound, u32>, // lengths in 18.2 Hz ticks
    speaker: Option<(Sound, i32)>, // sound playing and its remaining 18.2 Hz ticks
    flips: u32, // display page flips so far (original 0x13dd); the HUD item icons differ between the two pages
    item_calls: u32, // how many times the stage item has been drawn (original DS:0xcfb0 toggles A/B frames)
    pub levels: Vec<Level>,
    pub maps: Vec<Vec<Map>>,
    pub level_idx: usize,
    pub stage_idx: usize,
    pub player: Player,
    pub items: Items,
    pub collected: std::collections::HashSet<(usize, usize)>,
    pub enemies: Vec<Enemy>,
    pub fireballs: [Option<Fireball>; 5],
    pub max_fireballs: usize,
    pub hp: i32,          // 0..=6
    pub lives: i32,       // spare lives, 0..=5
    pub score: u32,
    score_hundreds: u32,
    pub meter: i32,       // fire energy 0..=12
    recharge: i32,
    refill: i32,          // ticks of pending HP refill
    pub treasures: i32,
    pub treasure_flags: [bool; 3],
    pub spawn: (i32, i32),
    resp_timer: i32,      // cf60
    spawn_off: i32,       // cf61
    spawned: bool,        // cf62
    pub tele: Option<Teleport>,
    pub dying: Option<u8>,
    pub game_over: bool,
    pub win_countdown: i32,
    pub won: bool,
    pub ticks: u64,
    /// Enhancement: counts down while a "needs the Door Key" hint is on screen (the original shows nothing).
    pub hint: u32,
    /// Enhancement cheats (set by the host from the settings menu).
    pub cheat_fire: bool,
    pub cheat_health: bool,
    /// Cheat: carry every item (turning it off keeps what was given).
    pub cheat_items: bool,
    /// Counts respawns / stage jumps, so the host does not smooth a movement across them.
    pub resets: u32,
    /// Widescreen enhancement: fireballs may fly this many units beyond the original 24-unit window on each side.
    pub view_margin: i32,
}

fn sar3(v: i32) -> i32 { v >> 3 }

impl Game {
    pub fn new(levels: Vec<Level>, maps: Vec<Vec<Map>>, sound_len: HashMap<Sound, u32>) -> Game {
        let mut g = Game {
            phase: Phase::Opening(0), sounds: vec![], sound_len, speaker: None, flips: 0, item_calls: 0, levels, maps, level_idx: 1, stage_idx: 0, player: Player::new(12, 14), items: Items::default(),
            collected: Default::default(), treasure_flags: [false; 3], enemies: vec![], fireballs: [None; 5], max_fireballs: 0,
            hp: 0, lives: 4, score: 0, score_hundreds: 0, meter: 0, recharge: 2, refill: 6, treasures: 0,
            spawn: (12, 14), resp_timer: 0x14, spawn_off: 0x16, spawned: false, tele: None, dying: None,
            game_over: false, win_countdown: 0, won: false, ticks: 0, hint: 0, cheat_fire: false, cheat_health: false, cheat_items: false, resets: 0, view_margin: 0,
        };
        g.init_stage();
        snd!(g, Sound::Start);
        g
    }

    #[allow(dead_code)]
    pub fn spawn_counters(&self) -> (i32, i32) { (self.resp_timer, self.spawn_off) }
    /// The PC speaker as the original models it: a new sound is ignored while one of higher priority is still
    /// playing; otherwise it replaces the current one. Accepted sounds are queued for the host.
    fn play_fields(speaker: &mut Option<(Sound, i32)>, lens: &HashMap<Sound, u32>, out: &mut Vec<Sound>, s: Sound) {
        if let Some((cur, rem)) = *speaker { if rem > 0 && cur.priority() > s.priority() { return; } }
        let len = lens.get(&s).copied().unwrap_or(0) as i32;
        *speaker = if len > 0 { Some((s, len)) } else { None };
        out.push(s);
    }

    /// HUD item icons (inventory, cola count) are drawn with a different animation frame into each display page,
    /// so they blink as the pages flip: frame A on odd flips, B on even.
    pub fn hud_icon_frame(&self) -> usize { if self.flips % 2 == 1 { 0 } else { 1 } }

    /// Which of the item's two animation frames is on screen: it alternates every time the item is drawn (0x20a0).
    pub fn item_frame(&self) -> usize { if self.item_calls % 2 == 1 { 0 } else { 1 } }

    /// The stage item is drawn (and its animation advances) whenever it exists, is not collected and is on screen.
    fn item_drawn(&mut self) {
        if let Some((_, _, ix)) = self.stage().item {
            let sx = ix as i32 - self.player.cam;
            if !self.collected.contains(&(self.level_idx, self.stage_idx)) && (0..=0x16).contains(&sx) { self.item_calls += 1; }
        }
    }

    /// True while some sound is still playing (the original's INT 3 service 4).
    #[allow(dead_code)]
    pub fn sound_playing(&self) -> bool { self.speaker.is_some() }

    pub fn map(&self) -> &Map { &self.maps[self.level_idx][self.stage_idx] }
    pub fn stage(&self) -> &Stage { &self.levels[self.level_idx].stages[self.stage_idx] }

    fn init_stage(&mut self) {
        let old = std::mem::take(&mut self.enemies);
        self.fireballs = [None; 5];
        self.tele = None;
        let level = &self.levels[self.level_idx];
        self.enemies = self.levels[self.level_idx].stages[self.stage_idx].enemies.iter().filter(|(s, _)| *s != 0xff).map(|&(sprite, b)| {
            let def = level.sprites.get(sprite as usize);
            let count = def.map_or(1, |d| d.frames + d.extra);
            Enemy { y: 0, x: 0, dx: 0, vy: 0, frame: 0, count, beh: b & 0x7f, state: 0, face: 0,
                    toggle: if b >= 0x80 { 2 } else { 0 }, timer: 0x14, sprite: sprite as usize }
        }).collect();
        for (i, e) in self.enemies.iter_mut().enumerate() {
            if let Some(o) = old.get(i) { e.y = o.y; e.x = o.x; e.dx = o.dx; e.vy = o.vy; }
        }
    }

    fn goto_stage(&mut self, level: usize, stage: usize, y: i32, x: i32, keep_state: bool) {
        self.resets += 1;
        self.level_idx = level;
        self.stage_idx = stage;
        if keep_state { self.player.relocate(y, x); } else { self.player = Player::new(y, x); }
        self.spawn = (y, x);
        self.init_stage();
    }

    pub fn add_score(&mut self, n: u32) {
        let before = self.score;
        self.score += n;
        let crossed = self.score / 100 - before / 100;
        self.score_hundreds += crossed;
        while self.score_hundreds >= 5 { self.score_hundreds -= 5; self.extra_life(); }
    }

    fn extra_life(&mut self) {
        snd!(self, Sound::ExtraLife);
        if self.lives < 5 { self.lives += 1; } else { self.refill = 6; self.add_score(0x4b * 3); }
    }

    fn collect(&mut self, kind: u8) {
        match kind {
            0 => self.items.corkscrew = true,
            1 => self.items.key = true,
            2 => self.items.boots = true,
            3 => self.items.lantern = true,
            4 => self.items.wand = true,
            5 | 6 | 7 => {
                self.extra_life();
                self.treasure_flags[(kind - 5) as usize] = true;
                self.treasures += 1;
                if self.treasures == 3 { self.win_countdown = 0x14; }
            }
            0xe => { if self.hp == 6 { self.extra_life(); } else { self.refill = 6; } }
            _ => { if self.max_fireballs < 5 { self.max_fireballs += 1; } }
        }
    }

    fn kill_player(&mut self) { self.kill_with(Sound::Death); }
    fn kill_with(&mut self, snd: Sound) { if self.dying.is_none() { self.dying = Some(9); snd!(self, snd); } }

    fn respawn(&mut self) {
        if self.lives == 0 { self.game_over = true; self.phase = Phase::GameOver; snd!(self, Sound::GameOver); return; }
        self.resets += 1;
        self.lives -= 1;
        self.refill = 6;
        self.recharge = 2;
        let facing = self.player.facing;
        self.player = Player::new(self.spawn.0, self.spawn.1);
        self.player.facing = facing;
        self.init_stage();
    }

    /// Restart the whole game (after game over / victory).
    pub fn new_game(&mut self) {
        let (levels, maps, lens) = (std::mem::take(&mut self.levels), std::mem::take(&mut self.maps), std::mem::take(&mut self.sound_len));
        *self = Game::new(levels, maps, lens);
    }

    /// One original game tick (~9.1 per second).
    pub fn tick(&mut self, inp: Input) {
        self.ticks += 1;
        if let Some((cur, rem)) = self.speaker { self.speaker = if rem > 2 { Some((cur, rem - 2)) } else { None }; }
        match self.phase {
            Phase::Opening(t) => {
                // 15 empty ticks, then wait until the start jingle has finished (original 0x11c7), then the
                // beam-in (12 frames, Comic from frame 6) and 6 ticks standing before play begins
                if t == 15 {
                    if self.speaker.is_some() { self.item_drawn(); self.flips += 1; return; }
                    snd!(self, Sound::Beam);
                }
                // every opening iteration draws the item: 15 empty ticks, the jingle wait, 12 beam frames, 1 final draw
                if t + 1 <= 28 { self.item_drawn(); self.flips += 1; }
                self.phase = if t + 1 >= 34 { Phase::Playing } else { Phase::Opening(t + 1) };
                return;
            }
            Phase::Door(d) => { self.door_tick(d); return; }
            Phase::Fall(t) => {
                let t = t + 1;
                if t == 1 { self.flips += 1; }
                if t == 3 { snd!(self, Sound::Fall); }
                if t >= 18 { self.phase = Phase::Playing; self.respawn(); } else { self.phase = Phase::Fall(t); }
                return;
            }
            Phase::Ending(e) => { self.ending_tick(e); return; }
            Phase::GameOver => return,
            Phase::Playing => {}
        }
        if let Some(n) = self.dying {
            if n <= 1 { self.dying = None; self.flips += 1; self.respawn(); } else { self.dying = Some(n - 1); self.enemies_tick(); self.flips += 1; }
            return;
        }
        if self.win_countdown > 0 {
            self.win_countdown -= 1;
            if self.win_countdown == 1 { self.won = true; self.phase = Phase::Ending(Ending::BeamOut(0)); snd!(self, Sound::Beam); return; }
        }
        // original idle loop (0x1249): on the ground with jump released, the jump counter is reloaded from [0x48]
        // (4, or 5 with the boots) between ticks, whatever else (e.g. a teleport animation) is going on
        if !self.player.air && !inp.jump { self.player.reload_jump(if self.items.boots { 5 } else { 4 }); }
        if self.cheat_health { self.hp = 6; }
        if self.cheat_items {
            self.items = Items { corkscrew: true, key: true, boots: true, lantern: true, wand: true };
            self.max_fireballs = self.max_fireballs.max(5);
        }
        if self.cheat_fire { self.meter = 12; }
        self.player.begin_tick();
        if self.refill > 0 {
            self.refill -= 1;
            // at full health the original pays points instead; not while the infinite-health cheat is on (every death would pay)
            if self.hp == 6 { if !self.cheat_health { self.add_score(0x12); } } else { self.hp += 1; }
        }

        if self.tele.is_some() {
            self.teleport_step();
        } else {
            let (level, stage) = (self.level_idx, self.stage_idx);
            self.hint = self.hint.saturating_sub(1);
            if inp.open && !self.items.key && !self.player.air && self.at_door() { self.hint = 18; }
            let ev = {
                let map = &self.maps[level][stage];
                let st = &self.levels[level].stages[stage];
                self.player.tick(inp, map, st, level, &self.items)
            };
            match ev {
                Event::None => {}
                Event::Fell => {
                    // original 0x142f: Comic sinks out of the bottom of the screen (one frame), the empty playfield
                    // shows for 2 ticks, the fall sound plays, 15 more ticks pass, then the life is lost
                    self.phase = Phase::Fall(0);
                    if self.player.y <= 19 { self.flips += 1; }
                    return;
                }
                Event::Teleport => { self.start_teleport(); self.teleport_step(); self.enemies_tick(); if self.dying.is_some() { return; } self.fireballs_tick(); self.item_tick(); self.flips += 1; return; }
                Event::ChangeStage { stage: s, y, x } => { self.player.stop_vertical(); snd!(self, Sound::StageChange); self.goto_stage(level, s, y, x, true); return; }
                Event::EnterDoor { level: nl, stage: ns, y: dy, x: dx } => {
                    let doors = &self.levels[nl].stages[ns].doors;
                    let d = doors.iter().find(|d| d.level as usize == level && d.stage as usize == stage).or(doors.first());
                    let (y, x) = d.map_or((12, 14), |d| (d.y as i32, d.x as i32 + 1));
                    snd!(self, Sound::Door);
                    self.flips += 1;
                    let cam = self.player.cam;
                    self.phase = Phase::Door(DoorAnim {
                        arriving: false, frame: 0, level, stage, cam, old: (level, stage, cam),
                        door: (dy, dx), pending: Some((nl, ns, y, x)),
                    });
                    return;
                }
            }
            self.fire(inp.fire);
        }

        self.enemies_tick();
        if self.dying.is_some() { return; }
        self.fireballs_tick();
        self.item_tick();
        self.flips += 1;
    }

    /// One tick of the door animation. Leaving takes 4 ticks, then the stage is loaded and arriving takes 8.
    fn door_tick(&mut self, mut d: DoorAnim) {
        if !d.arriving {
            if d.frame + 1 >= 4 {
                let Some((nl, ns, y, x)) = d.pending else { self.phase = Phase::Playing; return };
                let old = (d.level, d.stage, d.cam);
                self.goto_stage(nl, ns, y, x, true);
                self.phase = Phase::Door(DoorAnim {
                    arriving: true, frame: 0, level: nl, stage: ns, cam: self.player.cam, old,
                    door: (y, x - 1), pending: None,
                });
            } else {
                d.frame += 1;
                self.flips += 1;
                self.phase = Phase::Door(d);
            }
        } else {
            d.frame += 1;
            if d.frame == 3 { snd!(self, Sound::Door); }
            if (3..=7).contains(&d.frame) { self.flips += 1; }
            self.phase = if d.frame >= 8 { Phase::Playing } else { Phase::Door(d) };
        }
    }

    fn ending_tick(&mut self, e: Ending) {
        use Ending::*;
        self.phase = Phase::Ending(match e {
            BeamOut(t) => if t + 1 >= 18 { Bonus(0) } else { BeamOut(t + 1) },
            Bonus(t) => { self.add_score(10); snd!(self, Sound::Bonus); if t + 1 >= 20 { LifeBonus(0) } else { Bonus(t + 1) } }
            LifeBonus(t) => {
                if self.lives == 0 { snd!(self, Sound::Title); Screen(0) } else if t < 10 { self.add_score(10); snd!(self, Sound::Bonus); LifeBonus(t + 1) }
                else if t == 10 { self.lives -= 1; LifeBonus(t + 1) }
                else if t >= 13 { LifeBonus(0) } else { LifeBonus(t + 1) }
            }
            Screen(t) => Screen(t.saturating_add(1)),
            Banner => Banner,
        });
    }

    /// Host calls this when the player presses a key during the ending screens.
    pub fn ending_key(&mut self) -> bool {
        match self.phase {
            Phase::Ending(Ending::Screen(t)) if t >= 25 => { self.phase = Phase::Ending(Ending::Banner); false } // fade-in (5 ticks, 0x0340) + 20 ticks (0x1110)
            Phase::Ending(Ending::Banner) | Phase::GameOver => true, // finished: host shows the high-score table
            _ => false,
        }
    }

    // ---- fire / fireballs ---------------------------------------------------------------
    fn fire(&mut self, key: bool) {
        let mut skip_recharge = false;
        if key {
            if self.meter == 0 {
                return; // original skips the recharge too when the meter is empty
            }
            let slots = self.fire_slots();
            if slots > 0 {
                let (y, x) = (self.player.y + 1, self.player.x);
                let dx = if self.player.facing == 0 { 2 } else { -2 };
                if let Some(slot) = self.fireballs.iter_mut().take(slots).find(|f| f.is_none()) {
                    *slot = Some(Fireball { y, x, dx, ctr: 2, frame: 0 });
                    snd!(self, Sound::Fire);
                }
            }
            if self.recharge == 1 { self.recharge = 2; skip_recharge = true; } else { self.meter -= 1; }
        }
        if !skip_recharge {
            self.recharge -= 1;
            if self.recharge == 0 { self.meter = (self.meter + 1).min(12); self.recharge = 2; }
        }
    }

    /// Fireballs that may be in flight at once (the unlimited-fireballs cheat allows all five).
    /// How far the visible view reaches beyond the original 24-unit window on each side: `(left, right)`. In a wide view
    /// (view_margin > 0) the view is centred on the window but cannot show anything left of the stage start or right of
    /// its end, so next to the stage's ends it shifts and one side shows more than `view_margin`.
    fn view_extra(&self) -> (i32, i32) {
        let wu = self.view_margin;
        if wu <= 0 { return (0, 0); }
        let cam = self.player.cam;
        let width = 24 + 2 * wu;
        let map_units = self.maps[self.level_idx][self.stage_idx].w as i32 * 2;
        let left = (cam - wu).clamp(0, (map_units - width).max(0));
        (cam - left, left + width - (cam + 24))
    }

    fn fire_slots(&self) -> usize { if self.cheat_fire { 5 } else { self.max_fireballs } }

    fn fireballs_tick(&mut self) {
        let cam = self.player.cam;
        let slots = self.fire_slots();
        let (lm, rm) = self.view_extra();
        for slot in self.fireballs.iter_mut().take(slots) {
            let Some(mut f) = *slot else { continue };
            f.x += f.dx;
            let sx = f.x - cam;
            // the original culls a fireball at 22 units; a wider view reaches 24 + margin, so use all of it
            let right = if self.view_margin > 0 { 0x18 + rm } else { 0x16 };
            if f.x < 0 || sx < -lm || sx > right { *slot = None; continue; }
            if self.items.corkscrew {
                f.ctr -= 1;
                if f.ctr == 0 { f.y -= 1; f.ctr = 2; } else { f.y += 1; }
            }
            f.frame = (f.frame + 1) % 2;
            *slot = Some(f);
        }
        // hits
        let mut gained = 0;
        for slot in self.fireballs.iter_mut().take(slots) {
            let Some(f) = *slot else { continue };
            for e in self.enemies.iter_mut().filter(|e| e.state == 1) {
                let dy = f.y - e.y;
                // 0x1d0f: |fx-ex| is a byte compared with a *signed* JLE, so distances of 128+ count as hits
                let dist = (f.x - e.x).unsigned_abs() as u8;
                if (0..=1).contains(&dy) && (dist as i8) <= 1 {
                    e.state = 2;
                    *slot = None;
                    gained += 3;
                    snd!(self, Sound::EnemyHit);
                    break;
                }
            }
        }
        if gained > 0 { self.add_score(gained); }
    }

    // ---- items -------------------------------------------------------------------------
    fn item_tick(&mut self) {
        if let Some((kind, iy, ix)) = self.stage().item {
            let key = (self.level_idx, self.stage_idx);
            if !self.collected.contains(&key) && touches_item(&self.player, iy, ix) {
                self.collected.insert(key);
                snd!(self, Sound::Item);
                self.add_score(20);
                self.collect(kind);
            } else {
                self.item_drawn();
            }
        }
    }

    // ---- teleport wand -----------------------------------------------------------------
    fn start_teleport(&mut self) {
        snd!(self, Sound::Teleport);
        let p = &self.player;
        let (mut ax, ay) = (p.x, p.y);
        let mut bl = p.x - p.cam; // screen-relative x
        let mut dl = p.cam;
        let (dir, mut scroll);
        let ok;
        if p.facing != 0 {
            dir = -1; scroll = 0;
            bl -= 6;
            if bl < 0 { ok = false; } else {
                ax -= 6;
                bl -= 10;
                if bl < 0 {
                    let mut amt = -bl;
                    dl -= amt;
                    if dl < 0 { amt += dl; }
                    scroll = amt;
                }
                ok = true;
            }
        } else {
            dir = 1; scroll = 0;
            if bl >= 0x11 { ok = false; } else {
                ax += 6;
                bl += 6;
                bl -= 12;
                if bl > 0 {
                    // original 0x0ef8: byte arithmetic with a *signed* compare (JLE) against 0xe8. For a camera
                    // below 128 the compare is always "greater", so the scroll distance wraps to a huge value.
                    let d = (dl as u8).wrapping_add(bl as u8);
                    let mut amt = bl as u8;
                    if (d as i8) > (0xe8u8 as i8) { amt = amt.wrapping_sub(d.wrapping_sub(0xe8)); }
                    scroll = amt as i32;
                }
                ok = true;
            }
        }
        let mut dest = (ay, p.x);
        if ok {
            let map = self.map();
            let col = ax & !1;
            let mut y = 0x12;
            let mut found = false;
            // go up while the tile is empty, then up through solid tiles, then need 2 empty rows
            while y > 0 && !map.solid(y, col) { y -= 2; }
            if y > 0 || map.solid(y, col) {
                if map.solid(y, col) {
                    let mut empties = 0;
                    loop {
                        y -= 2;
                        if y < 0 { break; }
                        if map.solid(y, col) { empties = 0; } else { empties += 1; if empties == 2 { found = true; break; } }
                    }
                    if !found { /* fail */ }
                }
            }
            if found { dest = (y, col); }
        }
        let orig = (self.player.y, self.player.x);
        self.tele = Some(Teleport { step: 0, orig, dest, scroll, scroll_dir: dir });
    }

    fn teleport_step(&mut self) {
        let Some(mut t) = self.tele else { return };
        if t.scroll > 0 {
            self.player.cam += t.scroll_dir; // the original never clamps this
            t.scroll -= 1;
        }
        if t.step == 3 { self.player.y = t.dest.0; self.player.x = t.dest.1; }
        t.step += 1;
        self.tele = if t.step >= 6 { None } else { Some(t) };
    }

    // ---- enemies -----------------------------------------------------------------------
    fn despawn(&mut self, i: usize) {
        let e = &mut self.enemies[i];
        e.state = 0;
        e.timer = self.resp_timer;
        self.resp_timer += 0x14;
        if self.resp_timer >= 0x78 { self.resp_timer = 0x14; }
    }

    fn try_spawn(&mut self, i: usize) {
        self.enemies[i].timer = 0;
        if self.spawned { return; }
        let cam = self.player.cam;
        let mut off = self.spawn_off + 2;
        if off >= 0x1f { off = 0x18; }
        self.spawn_off = off;
        let x;
        if self.player.facing != 0 {
            let a = ((off - 0x16) & 0xff) as i32;
            x = cam - a;
            if x < 0 { return; }
        } else {
            let (_, ov) = (cam as u8 as i8).overflowing_add(off as i8);
            if ov { return; }
            x = (cam + off) & 0xff; // byte arithmetic: wraps to the far left near the right edge
        }
        // 0x1b9e: scan upward from just below the player's feet through empty tiles until something solid is
        // found, then up through the solid tiles to the first empty one, and spawn on top of that structure.
        let map = &self.maps[self.level_idx][self.stage_idx];
        let mut y = (self.player.y & !1) + 4;
        loop {
            if map.solid(y, x) { break; }
            y -= 2;
            if y == 0 { return; }
        }
        loop {
            y -= 2;
            if y < 0 { return; }
            if !map.solid(y, x) { break; }
        }
        self.spawned = true;
        let e = &mut self.enemies[i];
        e.y = y; e.x = x; e.state = 1; e.frame = 0;
        match e.beh {
            1 | 5 => { e.dx = -1; e.vy = -1; }
            0x7f => { self.spawned = false; e.state = 0; e.timer = 0x64; }
            _ => { e.dx = 0; e.vy = 0; }
        }
    }

    fn enemies_tick(&mut self) {
        self.spawned = false;
        let was_dying = self.dying.is_some();
        for i in 0..self.enemies.len() {
            match self.enemies[i].state {
                0 => {
                    if self.enemies[i].timer > 0 { self.enemies[i].timer -= 1; } else { self.try_spawn(i); }
                }
                1 => {
                    let e = &mut self.enemies[i];
                    e.frame = (e.frame + 1) % e.count.max(1);
                    self.move_enemy(i);
                    self.enemy_contact(i);
                }
                _ => self.enemy_dying(i),
            }
            // the original jumps straight into its death sequence, abandoning the rest of this pass
            if !was_dying && self.dying.is_some() { return; }
        }
    }

    fn enemy_dying(&mut self, i: usize) {
        let st = self.enemies[i].state;
        if st == 7 { self.despawn(i); return; }
        let mut bl = st;
        if bl > 7 { bl -= 6; if bl == 7 { self.despawn(i); return; } }
        let sx = self.enemies[i].x - self.player.cam;
        let (lm, rm) = self.view_extra();
        if !(-lm..=0x16 + rm).contains(&sx) { self.despawn(i); return; }
        self.enemies[i].state += 1;
    }

    /// Distance/collision test, runs after movement (original 0x172b).
    fn enemy_contact(&mut self, i: usize) {
        let (ey, ex) = (self.enemies[i].y, self.enemies[i].x);
        // the original drops enemies 30 units from Comic; in a wider view (view_margin > 0) the limit grows with it, so
        // enemies do not vanish while still on screen
        let (lm, rm) = self.view_extra();
        if (ex - self.player.x).abs() >= 0x1e + lm.max(rm) { self.despawn(i); return; }
        let dy = ey - self.player.y;
        if (ex - self.player.x).abs() < 2 && (0..4).contains(&dy) {
            self.enemies[i].state = 8;
            if self.dying.is_none() && self.hp == 0 { self.kill_player(); } else if self.dying.is_none() { self.hp -= 1; snd!(self, Sound::Hurt); }
            self.enemy_dying(i);
        }
    }

    fn move_enemy(&mut self, i: usize) {
        let map = &self.maps[self.level_idx][self.stage_idx];
        let (px, py, cam, pface) = (self.player.x, self.player.y, self.player.cam, self.player.facing);
        let e = &mut self.enemies[i];
        let (mut y, mut x) = (e.y, e.x);
        // helpers mirroring 0x09fa (solid2: x odd -> also the next column) and 0x0a34 (solid1: y odd -> also the next row)
        let s2 = |y: i32, x: i32| map.solid2(y, x);
        // 0x0a34: a body whose y is odd straddles two tile rows, so the row below is checked too
        let s1 = |y: i32, x: i32| map.solid(y, x) || (y & 1 == 1 && map.solid(y + 2, x));
        // horizontal bounce shared by behaviours 1, 2 and 5 (turn at walls and at the screen edges)
        let bounce_x = |x: &mut i32, dx: &mut i32, face: Option<&mut usize>, y: i32| {
            if *dx == 1 {
                if let Some(f) = face { *f = 5; }
                if s1(y, *x + 2) { *dx = -1; } else {
                    *x += 1;
                    let bh = *x - cam;
                    if !(bh < 0 || bh < 0x16) { *dx = -1; }
                }
            } else {
                if let Some(f) = face { *f = 0; }
                if *x == 0 { *dx = 1; } else if s1(y, *x - 1) { *dx = 1; } else {
                    *x -= 1;
                    if *x <= cam { *dx = 1; }
                }
            }
        };
        match e.beh {
            1 => {
                if e.toggle == 1 { e.toggle = 0; return; }
                if e.toggle == 0 { e.toggle = 1; }
                let mut face = e.face;
                bounce_x(&mut x, &mut e.dx, Some(&mut face), y);
                e.face = face;
                if e.vy == 1 {
                    if y >= 0x12 { e.vy = -1; } else if s2(y + 2, x) { e.vy = -1; } else { y += 1; if y >= 0x12 { e.vy = -1; } }
                } else if y == 0 { e.vy = 1; } else if s2(y - 1, x) { e.vy = 1; } else { y -= 1; if y == 0 { e.vy = 1; } }
            }
            2 => {
                let vy = e.vy;
                let mut nvy = vy;
                if vy < 0 {
                    let up = -sar3(vy);
                    if y - up >= 0 && !s2(y - up, x) { y -= up; }
                    nvy = vy;
                } else if vy > 0 {
                    let ny = y + sar3(vy);
                    if ny >= 0x12 { e.state = 7; e.y = 0x12; e.x = x; return; }
                    if !s2(ny + 1, x) { y = ny; }
                } else if s2(y + 2, x) {
                    // on the ground: pick a direction toward the player and leap
                    e.dx = if px >= x { 1 } else { -1 };
                    e.vy = -7;
                    return;
                }
                nvy = (nvy + 2).min(0x17);
                e.vy = nvy;
                if e.toggle == 1 { e.toggle = 0; } else {
                    e.toggle += 1;
                    let mut dx = e.dx;
                    bounce_x(&mut x, &mut dx, None, y);
                    e.dx = dx;
                }
                if e.vy > 0 && s2(y + 3, x) { y = (y + 1) & !1; e.vy = 0; }
            }
            3 => {
                if e.vy > 0 {
                    y += 1;
                    if y >= 0x11 { e.state = 7; e.y = 0x12; e.x = x; return; }
                } else {
                    e.dx = match px.cmp(&x) { std::cmp::Ordering::Equal => 0, std::cmp::Ordering::Greater => 1, _ => -1 };
                }
                if e.toggle == 1 { e.toggle = 0; } else {
                    e.toggle += 1;
                    if e.dx == 0 { e.toggle = 0; } // 0x1a0a: no horizontal direction -> straight to 0x1a2b which clears the toggle
                    else if e.dx > 0 { if !s1(y, x + 2) { x += 1; } }
                    else if !s1(y, x - 1) { x -= 1; }
                }
                if s2(y + 3, x) {
                    if e.vy != 0 { y = (y + 1) & !1; e.vy = 0; }
                } else { e.vy = 1; }
            }
            4 => {
                if e.toggle == 1 { e.toggle = 0; e.y = y; e.x = x; return; }
                e.toggle += 1;
                let dxp = x - px;
                let mut moved = false;
                if dxp != 0 {
                    if dxp > 0 { e.face = 0; if !s1(y, x - 1) { x -= 1; moved = true; } }
                    else { e.face = 5; if !s1(y, x + 2) { x += 1; moved = true; } }
                }
                if !moved {
                    let dyp = y - py;
                    if dyp == 0 { e.toggle = 0; }
                    else if dyp > 0 { if !s2(y - 1, x) { y -= 1; } else { e.toggle = 0; } }
                    else if !s2(y + 2, x) { y += 1; } else { e.toggle = 0; }
                }
            }
            5 => {
                if e.toggle == 1 { e.toggle = 0; return; }
                if e.toggle == 0 { e.toggle = 1; }
                let mut face = e.face;
                bounce_x(&mut x, &mut e.dx, Some(&mut face), y);
                e.face = face;
                // vertical (0x1b0f): flee upward when the player faces the enemy, otherwise chase the player's row.
                // [+3] is written along the way (never read), mirrored here so internal state matches the original.
                let dxp = px - x;
                let mut go_up = dxp != 0 && ((dxp > 0 && pface == 5) || (dxp < 0 && pface == 0));
                if !go_up {
                    let d = py - y;
                    if d < 0 { go_up = true; }
                    else if d > 0 {
                        if y >= 0x12 { e.vy = -1; }
                        else if s2(y + 2, x) { e.vy = -1; }
                        else { y += 1; if y >= 0x12 { e.vy = -1; } }
                    }
                }
                if go_up {
                    if y == 0 { e.vy = 1; }
                    else if s2(y - 1, x) { e.vy = 1; }
                    else { y -= 1; if y == 0 { e.vy = 1; } }
                }
            }
            _ => return,
        }
        e.y = y;
        e.x = x;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Stage;

    fn flat_map() -> Map {
        // 128x10 map: solid floor at tile row 7 (units 14,15), empty above
        let mut cells = vec![0u8; 1280];
        for c in 0..128 { cells[7 * 128 + c] = 0x40; for r in 8..10 { cells[r * 128 + c] = 0x40; } }
        Map { cells, w: 128, h: 10, solid_above: 0x3b }
    }
    fn empty_stage() -> Stage { Stage { item: None, left: None, right: None, doors: vec![], enemies: vec![] } }

    #[test]
    fn standing_player_stays_put() {
        let m = flat_map(); let st = empty_stage();
        let mut p = Player::new(10, 20);
        for _ in 0..10 { p.tick(Input::default(), &m, &st, 1, &Items::default()); }
        assert_eq!((p.y, p.x, p.air), (10, 20, false));
    }

    #[test]
    fn jump_rises_seven_units_then_lands() {
        let m = flat_map(); let st = empty_stage();
        let mut p = Player::new(10, 20);
        let (mut top, mut landed_at) = (10, None);
        for t in 0..40 {
            p.tick(Input { jump: true, ..Default::default() }, &m, &st, 1, &Items::default());
            top = top.min(p.y);
            if !p.air && t > 2 { landed_at = Some(t); break; }
        }
        assert_eq!(top, 3, "apex should be 7 units above the start with a held jump");
        assert!(landed_at.is_some());
        assert_eq!(p.y, 10);
    }

    #[test]
    fn walks_one_unit_per_tick_after_turning() {
        let m = flat_map(); let st = empty_stage();
        let mut p = Player::new(10, 20);
        let r = Input { right: true, ..Default::default() };
        p.tick(r, &m, &st, 1, &Items::default()); // already faces right -> moves
        assert_eq!(p.x, 21);
        let l = Input { left: true, ..Default::default() };
        p.tick(l, &m, &st, 1, &Items::default()); // turn costs a tick
        assert_eq!(p.x, 21);
        p.tick(l, &m, &st, 1, &Items::default());
        assert_eq!(p.x, 20);
    }
    // ---- tests that use the real game data (skipped if the assets are not next to the repo) ----
    fn real_game() -> Option<Game> {
        use std::path::Path;
        let dir = Path::new("../AdvCapCo");
        if !dir.join("comic.exe").exists() { return None; }
        let exe = crate::exe::Exe::load(&dir.join("comic.exe")).ok()?;
        let levels: Vec<Level> = (0..crate::data::LEVEL_COUNT).map(|i| crate::data::level(&exe, i)).collect();
        let maps = levels.iter().map(|l| {
            let ts = crate::assets::load_tileset(&dir.join(&l.tileset)).unwrap();
            l.maps.iter().map(|m| {
                let m = crate::assets::load_map(&dir.join(m)).unwrap();
                Map { cells: m.cells, w: m.w, h: m.h, solid_above: ts.header[0] }
            }).collect()
        }).collect();
        let lens = crate::sound::ALL.iter().map(|&s| (s, crate::sound::total_ticks(&exe, s))).collect();
        let mut g = Game::new(levels, maps, lens);
        g.phase = Phase::Playing;
        g.hp = 6; g.refill = 0;
        Some(g)
    }

    #[test]
    fn warp_entrance_respawns_on_the_ground() {
        let Some(mut g) = real_game() else { return };
        for level in 0..crate::data::LEVEL_COUNT {
            g.warp_to_level(level);
            g.phase = Phase::Playing;
            g.lives = 3;
            g.kill_player();
            while g.dying.is_some() { g.tick(Input::default()); }
            assert_eq!(g.lives, 2, "level {level}: life should be spent");
            for _ in 0..5 { g.tick(Input::default()); }
            let (y, x) = (g.player.y, g.player.x);
            let map = &g.maps[g.level_idx][g.stage_idx];
            assert!(!g.player.air && (map.solid(y + 4, x) || map.solid(y + 4, x + 1)),
                "level {level}: respawned at y={y} x={x} without ground under Comic");
        }
    }

    fn live_enemy(y: i32, x: i32, beh: u8) -> Enemy {
        Enemy { y, x, dx: -1, vy: -1, frame: 0, count: 3, beh, state: 1, face: 0, toggle: 0, timer: 0, sprite: 0 }
    }

    #[test]
    fn fireball_kills_enemy_and_scores() {
        let Some(mut g) = real_game() else { return };
        g.max_fireballs = 1; g.meter = 12;
        let (py, px) = (g.player.y, g.player.x);
        g.enemies = vec![live_enemy(py + 1, px + 4, 1)];
        for _ in 0..4 { g.tick(Input { fire: true, ..Default::default() }); }
        assert!(g.enemies[0].state >= 2 || g.score >= 3, "fireball should have hit the enemy");
        assert!(g.score >= 3);
    }

    #[test]
    fn touching_an_enemy_costs_one_hp_and_explodes_it() {
        let Some(mut g) = real_game() else { return };
        let (py, px) = (g.player.y, g.player.x);
        g.enemies = vec![live_enemy(py, px, 7)]; // behaviour 7 = inert, stays on the player
        let hp = g.hp;
        g.tick(Input::default());
        assert_eq!(g.hp, hp - 1);
        assert!(g.enemies[0].state >= 8);
    }

    #[test]
    fn dying_at_zero_hp_costs_a_life_and_respawns() {
        let Some(mut g) = real_game() else { return };
        g.hp = 0;
        let (py, px) = (g.player.y, g.player.x);
        g.enemies = vec![live_enemy(py, px, 7)];
        let lives = g.lives;
        for _ in 0..12 { g.tick(Input::default()); }
        assert_eq!(g.lives, lives - 1);
        assert!(g.dying.is_none());
    }

    #[test]
    fn random_play_never_breaks_invariants() {
        let Some(mut g) = real_game() else { return };
        let mut seed = 12345u32;
        let mut rnd = move || { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); (seed >> 16) & 0xff };
        for level in 0..8 {
            g.debug_goto(level);
            g.items.key = true; g.items.wand = true; g.max_fireballs = 5;
            let mut held = Input::default();
            for t in 0..1500 {
                if t % 6 == 0 {
                    held = Input { left: rnd() < 90, right: rnd() > 130, jump: rnd() < 60, open: rnd() < 30, fire: rnd() < 80, teleport: rnd() < 8 };
                }
                g.tick(held);
                if g.game_over || g.won { g.new_game(); g.debug_goto(level); g.items.key = true; g.max_fireballs = 5; }
                assert!((0..=6).contains(&g.hp), "hp {}", g.hp);
                assert!(g.lives >= 0 && g.lives <= 5);
                assert!(g.player.x >= 0 && g.player.x <= 255 && g.player.y >= 0 && g.player.y <= 0x14, "pos {} {}", g.player.x, g.player.y);
                assert!(g.player.cam >= 0 && g.player.cam <= 0xe8);
                assert!(g.meter >= 0 && g.meter <= 12);
            }
        }
    }

    #[test]
    fn opening_waits_for_the_start_jingle_before_the_beam_in() {
        let Some(mut g) = real_game() else { return };
        g.phase = Phase::Opening(0);
        snd!(g, Sound::Start); // the jingle starts as the game does: 131 timer ticks = ~66 game ticks
        let mut held_at_15 = 0;
        let mut ticks = 0;
        while g.phase != Phase::Playing && ticks < 300 {
            if g.phase == Phase::Opening(15) { held_at_15 += 1; }
            g.tick(Input::default());
            ticks += 1;
        }
        assert_eq!(g.phase, Phase::Playing);
        assert!(held_at_15 > 40, "waited {held_at_15} ticks for the jingle");
        assert!((70..=110).contains(&ticks), "opening took {ticks} ticks (15 + jingle wait + 12 beam + 6)");
        assert!(g.sounds.contains(&Sound::Beam));
    }

    #[test]
    fn higher_priority_sounds_are_not_interrupted_by_lower_ones() {
        let Some(mut g) = real_game() else { return };
        g.sounds.clear();
        snd!(g, Sound::Death);    // priority 4
        snd!(g, Sound::Fire);     // priority 0: ignored while the death sound plays
        assert_eq!(g.sounds, vec![Sound::Death]);
        for _ in 0..40 { g.tick(Input::default()); } // the death sound (9 ticks) is over
        snd!(g, Sound::Fire);
        assert_eq!(g.sounds.last(), Some(&Sound::Fire));
    }

    #[test]
    fn saving_is_allowed_whenever_comic_is_just_playing() {
        let Some(mut g) = real_game() else { return };
        g.new_game();
        let mut refused = vec![];
        for t in 0..200 {
            g.tick(Input::default());
            if !g.can_save() { refused.push((t, format!("{:?}", g.phase))); }
        }
        // only the opening animation may refuse; after it, a standing Comic can always save
        assert!(refused.iter().all(|(t, _)| *t < 90), "{refused:?}");
        g.goto_stage(4, 0, 12, 40, false);
        for _ in 0..90 { g.tick(Input::default()); }
        assert!(g.can_save(), "phase {:?}", g.phase);
    }

    #[test]
    fn loading_after_game_over_or_the_ending_allows_saving_again() {
        let Some(mut g) = real_game() else { return };
        for _ in 0..100 { g.tick(Input::default()); }
        let text = g.save_text();
        g.game_over = true; g.phase = Phase::GameOver;
        assert!(!g.can_save());
        assert!(g.load_text(&text));
        assert!(g.can_save(), "saving must work after loading a save over a game over");
        g.won = true; g.win_countdown = 5; g.phase = Phase::Ending(Ending::BeamOut(0));
        assert!(g.load_text(&text));
        assert!(g.can_save());
        assert_eq!(g.win_countdown, 0);
    }

    #[test]
    fn saving_is_never_blocked_for_long_during_random_play() {
        let Some(mut g) = real_game() else { return };
        let mut seed = 12345u32;
        let mut rnd = move || { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); (seed >> 16) as u32 };
        for level in 0..8 {
            g.new_game();
            for _ in 0..100 { g.tick(Input::default()); }
            g.warp_to_level(level);
            g.cheat_health = true; g.cheat_items = true; g.cheat_fire = true;
            let mut blocked = 0;
            for t in 0..6000 {
                // hold one random action set for a few ticks, like a player would
                let r = rnd();
                let inp = Input { left: r & 1 != 0, right: r & 2 != 0, jump: r & 4 != 0, fire: r & 8 != 0, open: r & 16 != 0, teleport: r % 53 == 0 };
                for _ in 0..(1 + rnd() % 4) {
                    g.tick(inp);
                    blocked = if g.can_save() || g.game_over || g.won { 0 } else { blocked + 1 };
                    assert!(blocked < 120, "level {level} tick {t}: saving blocked for {blocked} ticks: phase {:?} dying {:?} tele {:?}", g.phase, g.dying, g.tele.is_some());
                }
            }
        }
    }

    #[test]
    fn save_and_load_restore_the_checkpoint() {
        let Some(mut g) = real_game() else { return };
        g.items.key = true; g.items.boots = true; g.max_fireballs = 3; g.score = 4242; g.hp = 4; g.lives = 2;
        g.treasure_flags = [false, true, false]; g.treasures = 1; g.collected.insert((1, 2));
        g.goto_stage(3, 1, 12, 40, false);
        let text = g.save_text();
        let (key, score, lives, collected) = (g.items.key, g.score, g.lives, g.collected.clone());
        g.new_game();
        assert!(!g.items.key);
        assert!(g.load_text(&text), "{text}");
        assert_eq!((g.level_idx, g.stage_idx, g.player.y, g.player.x), (3, 1, 12, 40));
        assert_eq!((g.items.key, g.items.boots, g.items.wand, g.score, g.lives, g.hp), (key, true, false, score, lives, 4));
        assert_eq!((g.max_fireballs, g.treasures, g.treasure_flags, &g.collected), (3, 1, [false, true, false], &collected));
        assert_eq!(g.save_text(), text, "saving again gives the same file");
        assert!(!g.load_text("level=99\nstage=0\ny=1\nx=1\n"), "bad saves are refused");
        assert!(!g.load_text("nonsense"));
    }

    #[test]
    fn cheat_fireballs_all_fly_and_the_cheat_leaves_no_trace() {
        let Some(mut g) = real_game() else { return };
        while g.phase != Phase::Playing { g.tick(Input::default()); }
        g.max_fireballs = 1;
        g.cheat_fire = true;
        let fire = Input { fire: true, ..Default::default() };
        for _ in 0..6 { g.tick(fire); }
        let positions: Vec<i32> = g.fireballs.iter().flatten().map(|f| f.x).collect();
        assert!(positions.len() >= 3, "more than the earned slot are in flight: {positions:?}");
        g.tick(Input::default());
        let moved: Vec<i32> = g.fireballs.iter().flatten().map(|f| f.x).collect();
        assert!(moved.iter().all(|x| !positions.contains(x)) || moved.len() < positions.len(), "all of them move");
        assert_eq!(g.max_fireballs, 1, "the earned count is untouched");
    }

    #[test]
    fn a_full_health_refill_pays_points_except_with_the_infinite_health_cheat() {
        let Some(mut g) = real_game() else { return };
        while g.phase != Phase::Playing { g.tick(Input::default()); }
        g.hp = 6;
        g.refill = 6;
        let before = g.score;
        for _ in 0..6 { g.tick(Input::default()); }
        assert_eq!(g.score, before + 6 * 0x12, "the original's bonus at full health");
        g.cheat_health = true;
        g.refill = 6;
        let before = g.score;
        for _ in 0..6 { g.tick(Input::default()); }
        assert_eq!(g.score, before, "no bonus with the cheat");
    }

    #[test]
    fn a_wider_view_keeps_enemies_and_fireballs_until_the_edge() {
        let Some(mut g) = real_game() else { return };
        while g.phase != Phase::Playing { g.tick(Input::default()); }
        let (py, px) = (g.player.y, g.player.x);
        // an enemy 40 units away is dropped in the original view, kept in an ultrawide one
        for (margin, kept) in [(0, false), (17, true)] {
            g.view_margin = margin;
            g.enemies[0].state = 1;
            g.enemies[0].y = py;
            g.enemies[0].x = px + 40;
            g.enemy_contact(0);
            assert_eq!(g.enemies[0].state == 1, kept, "margin {margin}");
        }
    }

    #[test]
    fn the_wide_view_extra_follows_the_stage_ends() {
        let Some(mut g) = real_game() else { return };
        while g.phase != Phase::Playing { g.tick(Input::default()); }
        g.view_margin = 17;
        g.player.cam = 0;
        assert_eq!(g.view_extra(), (0, 34), "at the stage start everything extra is on the right");
        g.player.cam = 40;
        assert_eq!(g.view_extra(), (17, 17), "in the middle it is symmetrical");
        g.view_margin = 0;
        assert_eq!(g.view_extra(), (0, 0), "faithful view");
    }

    #[test]
    fn a_shot_enemy_passes_through_state_7_for_one_tick_before_it_is_removed() {
        let Some(mut g) = real_game() else { return };
        while g.phase != Phase::Playing { g.tick(Input::default()); }
        g.enemies[0].state = 2;
        g.enemies[0].x = g.player.cam + 10;
        let mut seen = vec![];
        for _ in 0..8 { g.enemy_dying(0); seen.push(g.enemies[0].state); }
        assert_eq!(&seen[..6], &[3, 4, 5, 6, 7, 0], "explosion frames, one tick in state 7, then gone: {seen:?}");
    }

    #[test]
    fn a_locked_door_shows_a_hint_only_without_the_key() {
        let Some(mut g) = real_game() else { return };
        let open = Input { open: true, ..Default::default() };
        while g.phase != Phase::Playing { g.tick(Input::default()); }
        g.tick(open);
        assert!(g.hint > 0 && g.phase == Phase::Playing, "no key: stays put, hint shown");
        for _ in 0..30 { g.tick(Input::default()); }
        assert_eq!(g.hint, 0);
        g.items.key = true;
        g.tick(open);
        assert_eq!(g.hint, 0);
        assert!(matches!(g.phase, Phase::Door(_)));
    }

    #[test]
    fn door_animation_runs_4_ticks_out_then_8_ticks_in_and_changes_the_stage() {
        let Some(mut g) = real_game() else { return };
        // forest stage 0 has a door at (y=12, x=12); the start position (14) is within reach of it
        g.items.key = true;
        let open = Input { open: true, ..Default::default() };
        g.tick(open);
        let Phase::Door(d) = g.phase else { panic!("expected the door animation, got {:?}", g.phase) };
        assert!(!d.arriving);
        assert_eq!((g.level_idx, g.stage_idx), (1, 0), "still in the old stage while the door opens");
        let mut n = 1;
        while matches!(g.phase, Phase::Door(_)) && n < 40 {
            if let Phase::Door(d) = g.phase { if d.arriving && g.level_idx == 6 { break; } }
            g.tick(Input::default()); n += 1;
        }
        assert_eq!(g.level_idx, 6, "now in the castle");
        assert_eq!(n, 5, "4 door frames (ticks 1-4), then the stage loads on tick 5");
        let mut m = 0;
        while matches!(g.phase, Phase::Door(_)) && m < 40 { g.tick(Input::default()); m += 1; }
        assert_eq!(g.phase, Phase::Playing);
        assert_eq!(m, 8, "3 ticks waiting on the old scene + 5 arrival frames");
        // no enemies move and nothing is lost while the doors animate
        assert!(g.sounds.iter().filter(|s| **s == Sound::Door).count() == 2, "door sound on leaving and arriving");
    }

    #[test]
    fn ending_pays_bonus_per_spare_life_then_shows_the_story() {
        let Some(mut g) = real_game() else { return };
        g.phase = Phase::Ending(Ending::BeamOut(0));
        g.lives = 2; g.score = 0;
        for _ in 0..400 {
            g.tick(Input::default());
            if matches!(g.phase, Phase::Ending(Ending::Screen(_))) { break; }
        }
        assert!(matches!(g.phase, Phase::Ending(Ending::Screen(_))));
        assert_eq!(g.lives, 0, "every spare life is cashed in");
        assert_eq!(g.score, 20 * 10 + 2 * 10 * 10, "200 base bonus + 100 per life (in score units of 10)");
        assert!(!g.ending_key(), "key is ignored until the story screen has been up for a moment");
        for _ in 0..25 { g.tick(Input::default()); }
        assert!(!g.ending_key()); // moves on to the banner
        assert_eq!(g.phase, Phase::Ending(Ending::Banner));
        assert!(g.ending_key()); // finished: host restarts
    }

    #[test]
    fn game_over_after_last_life() {
        let Some(mut g) = real_game() else { return };
        g.phase = Phase::Playing;
        g.lives = 0; g.hp = 0;
        let (py, px) = (g.player.y, g.player.x);
        g.enemies = vec![live_enemy(py, px, 7)];
        for _ in 0..14 { g.tick(Input::default()); }
        assert_eq!(g.phase, Phase::GameOver);
    }

    #[test]
    fn falling_out_of_the_screen_plays_the_fall_sequence_then_costs_a_life() {
        let Some(mut g) = real_game() else { return };
        g.sounds.clear();
        g.player.air = true; g.player.y = 16; g.player.vy = 23;
        let lives = g.lives;
        g.tick(Input::default());
        assert!(matches!(g.phase, Phase::Fall(0)), "{:?}", g.phase);
        assert_eq!(g.lives, lives, "no life lost yet");
        let mut n = 0;
        while matches!(g.phase, Phase::Fall(_)) && n < 60 {
            g.tick(Input::default()); n += 1;
            if n == 2 { assert!(!g.sounds.contains(&Sound::Fall), "sound comes after the empty playfield"); }
        }
        assert_eq!(n, 18, "1 sinking frame + 2 empty + 15 wait");
        assert!(g.sounds.contains(&Sound::Fall));
        assert_eq!(g.phase, Phase::Playing);
        assert_eq!(g.lives, lives - 1);
        assert_eq!((g.player.y, g.player.x), g.spawn);
    }

}

impl Game {
    /// Debug helper: jump to the start of a level.
    pub fn debug_goto(&mut self, level: usize) { self.goto_stage(level, 0, 12, 14, false); }

    /// Where play really enters a level: in front of a door that leads to another level (arriving through a door puts
    /// Comic at the door's x+1 and makes that the respawn point, original 0x090d). The forest is entered at the start.
    pub fn level_entry(&self, level: usize) -> (usize, i32, i32) {
        if level == 1 { return (0, 12, 14); }
        for (s, st) in self.levels[level].stages.iter().enumerate() {
            if let Some(d) = st.doors.iter().find(|d| d.level as usize != level && (d.level as usize) < self.levels.len()) {
                return (s, d.y as i32, d.x as i32 + 1);
            }
        }
        (0, 12, 14)
    }

    /// Level skip for the player (Tab): like debug_goto, but arriving at the level's real entrance, so the respawn point
    /// is on the ground as in normal play.
    pub fn warp_to_level(&mut self, level: usize) {
        let (s, y, x) = self.level_entry(level);
        self.goto_stage(level, s, y, x, false);
    }

    /// Standing at a door (the same test the original uses to open it, without needing the key).
    fn at_door(&self) -> bool {
        let p = &self.player;
        self.levels[self.level_idx].stages[self.stage_idx].doors.iter().any(|d| p.y == d.y as i32 && ((p.x - d.x as i32) as u8) < 3)
    }

    /// Enhancement: a checkpoint save (level, stage, where Comic stands, what he carries and his score).
    /// Enemies start fresh when it is loaded, exactly as when entering a stage.
    pub fn save_text(&self) -> String {
        let i = &self.items;
        let b = |v: bool| v as u8;
        let mut collected: Vec<_> = self.collected.iter().collect();
        collected.sort();
        let col: Vec<String> = collected.iter().map(|(l, s)| format!("{l}:{s}")).collect();
        format!("level={}\nstage={}\ny={}\nx={}\nhp={}\nlives={}\nscore={}\nhundreds={}\nmeter={}\nfireballs={}\ntreasures={}\ntreasure_flags={}{}{}\nitems={}{}{}{}{}\ncollected={}\n",
            self.level_idx, self.stage_idx, self.player.y, self.player.x, self.hp, self.lives, self.score, self.score_hundreds,
            self.meter, self.max_fireballs, self.treasures, b(self.treasure_flags[0]), b(self.treasure_flags[1]), b(self.treasure_flags[2]),
            b(i.corkscrew), b(i.key), b(i.boots), b(i.lantern), b(i.wand), col.join(","))
    }

    pub fn can_save(&self) -> bool { self.phase == Phase::Playing && self.dying.is_none() && self.tele.is_none() && !self.game_over && !self.won }

    /// Restore a `save_text`. False (and nothing changed) when the text is not a valid save for these levels.
    pub fn load_text(&mut self, text: &str) -> bool {
        let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('='))).map(str::trim);
        let num = |k: &str| get(k).and_then(|v| v.parse::<i64>().ok());
        let (Some(level), Some(stage), Some(y), Some(x)) = (num("level"), num("stage"), num("y"), num("x")) else { return false };
        let (level, stage) = (level as usize, stage as usize);
        if level >= self.levels.len() || stage >= self.levels[level].stages.len() { return false; }
        let bits = |k: &str, n: usize| -> Option<Vec<bool>> { let v = get(k)?; (v.len() == n).then(|| v.chars().map(|c| c == '1').collect()) };
        let (Some(tf), Some(it)) = (bits("treasure_flags", 3), bits("items", 5)) else { return false };
        let mut collected = std::collections::HashSet::new();
        for part in get("collected").unwrap_or("").split(',').filter(|p| !p.is_empty()) {
            let Some((l, s)) = part.split_once(':') else { return false };
            let (Ok(l), Ok(s)) = (l.parse(), s.parse()) else { return false };
            collected.insert((l, s));
        }
        self.goto_stage(level, stage, y as i32, x as i32, false);
        self.hp = num("hp").unwrap_or(6).clamp(0, 6) as i32;
        self.lives = num("lives").unwrap_or(4).clamp(0, 5) as i32;
        self.score = num("score").unwrap_or(0).max(0) as u32;
        self.score_hundreds = num("hundreds").unwrap_or(0).max(0) as u32 % 5;
        self.meter = num("meter").unwrap_or(0).clamp(0, 12) as i32;
        self.max_fireballs = num("fireballs").unwrap_or(0).clamp(0, 5) as usize;
        self.treasures = num("treasures").unwrap_or(0).clamp(0, 3) as i32;
        self.treasure_flags = [tf[0], tf[1], tf[2]];
        self.items = Items { corkscrew: it[0], key: it[1], boots: it[2], lantern: it[3], wand: it[4] };
        self.collected = collected;
        self.dying = None;
        // a save loaded after a game over or the ending starts a fresh run: nothing of the old one may linger
        self.game_over = false;
        self.won = false;
        self.win_countdown = 0;
        self.phase = Phase::Playing;
        true
    }
}
