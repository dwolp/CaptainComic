#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")] // no console window behind the game
mod assets;
mod data;
mod exe;
mod fx;
mod game;
mod gamefiles;
mod hiscore;
mod input;
mod light;
mod menu;
mod pad;
mod settings;
mod sound;

use game::{DoorOp, Ending, Game, Input, Map, Phase, TICK_SECS};
use macroquad::audio::{load_sound_from_bytes, play_sound, stop_sound, PlaySoundParams};
use sound::Sound;
use std::collections::HashMap;
use macroquad::prelude::*;
use std::path::PathBuf;

const VIEW_W: f32 = 192.0; // playfield: 24 x 20 units of 8px
const VIEW_H: f32 = 160.0;
/// Widescreen: extra playfield width in px (8 units = 64 px on each side of the original window).
const WIDE_EXTRA: usize = 128;

/// CRT filter: the picture is rendered at CRT_K times the 320x200 size, then drawn through this shader.
const CRT_K: f32 = 4.0;
const CRT_FLIP: bool = true; // render targets are upside down
const CRT_VERTEX: &str = "#version 100
attribute vec3 position; attribute vec2 texcoord; attribute vec4 color0;
varying lowp vec2 uv; varying lowp vec4 color;
uniform mat4 Model; uniform mat4 Projection;
void main() { gl_Position = Projection * Model * vec4(position, 1.0); color = color0 / 255.0; uv = texcoord; }";
const CRT_FRAGMENT: &str = "#version 100
precision mediump float;
varying vec2 uv; varying vec4 color;
uniform sampler2D Texture;
uniform float TexW; uniform float Lines;
void main() {
    vec2 c = uv * 2.0 - 1.0;
    c *= 1.0 + dot(c, c) * 0.02;            // slight screen curvature
    vec2 p = c * 0.5 + 0.5;
    if (p.x < 0.0 || p.x > 1.0 || p.y < 0.0 || p.y > 1.0) { gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0); return; }
    float dx = 1.0 / TexW * 2.0;
    vec3 col = texture2D(Texture, p).rgb * 0.5
             + texture2D(Texture, p + vec2(dx, 0.0)).rgb * 0.25
             + texture2D(Texture, p - vec2(dx, 0.0)).rgb * 0.25;       // soft horizontal bleed
    col += col * col * 0.25;                                         // phosphor glow on bright areas
    float line = 0.5 + 0.5 * cos(6.2831853 * (p.y * Lines - 0.5));  // one scanline per virtual row
    col *= 0.62 + 0.38 * line;
    float mask = 0.94 + 0.06 * cos(6.2831853 * p.x * TexW / 4.0 / 3.0 * 3.0); // faint phosphor mask
    col *= mask;
    col *= 1.0 - 0.35 * dot(c, c) * 0.5;                              // vignette
    gl_FragColor = vec4(col, 1.0) * color;
}";

/// The window frame with the playfield made `extra` px wider: above y=176 a column inside the playfield is repeated,
/// below it a plain grey column of the bottom strip (so the score panel and the lives box keep their shape).
fn widen_frame(img: &assets::Image, extra: usize) -> assets::Image {
    let (w, h) = (img.w, img.h);
    let mut rgba = Vec::with_capacity((w + extra) * h * 4);
    for y in 0..h {
        let row = &img.rgba[y * w * 4..(y + 1) * w * 4];
        // (column, how many to add): the playfield grows; in the bottom strip the lives box ends up centred
        let cuts: [(usize, usize); 2] = if y < 176 { [(104, extra), (104, 0)] } else { [(20, extra / 2), (172, extra - extra / 2)] };
        let mut at = 0;
        for (col, n) in cuts {
            rgba.extend_from_slice(&row[at * 4..col * 4]);
            for _ in 0..n { rgba.extend_from_slice(&row[col * 4..col * 4 + 4]); }
            at = col;
        }
        rgba.extend_from_slice(&row[at * 4..]);
    }
    assets::Image { w: w + extra, h, rgba }
}

fn tex(img: &assets::Image) -> Texture2D {
    let t = Texture2D::from_rgba8(img.w as u16, img.h as u16, &img.rgba);
    t.set_filter(FilterMode::Nearest);
    t
}

fn window_conf() -> Conf {
    Conf { window_title: "Captain Comic Remastered".into(), window_width: 960, window_height: 760, ..Default::default() }
}

struct Sprites {
    player: Vec<Texture2D>,
    items: [Vec<Texture2D>; 2],
    explosion: Vec<Texture2D>,
    fireball: Vec<Texture2D>,
    puff: Vec<Texture2D>,
    beam: Vec<Texture2D>,
    death: Vec<Texture2D>,
    digits: Vec<Texture2D>,
    pips: Vec<Texture2D>, // empty, half, full
    life: [Texture2D; 2], // on, off
    // the same HUD sprites with their background made transparent (black, or the frame's grey for the lives), for Immersive
    ov_digits: Vec<Texture2D>,
    ov_pips: Vec<Texture2D>,
    ov_life: [Texture2D; 2],
    banner: Texture2D,
    screens: Vec<Texture2D>, // sys000..sys005
    frame_wide: Texture2D,   // sys003 with the playfield stretched by WIDE_EXTRA px
    fades: [Vec<Texture2D>; 3], // title, story and ending-story fade-in steps
    pause: Texture2D,           // "paused" banner (DS:0xb5c4)
    hint: Texture2D,            // "You need the Door Key" in the BIOS font (set once the font is loaded)
    hiscore_bg: assets::Image,  // sys005: the "Famous Comics" frame
}

/// The original's fade-in (0x0327 / 0x0340) for the title and story screens: palette registers 2, 10 and 12 start
/// black, then step through dark grey, light grey, white and back to their colours, one game tick per step.
fn fade_steps(img: &assets::Image) -> Vec<Texture2D> {
    const G: [u8; 3] = [85, 85, 85];
    const L: [u8; 3] = [170, 170, 170];
    const W: [u8; 3] = [255, 255, 255];
    let steps: [[[u8; 3]; 3]; 4] = [[G, G, G], [L, L, L], [L, W, W], [assets::EGA[2], assets::EGA[10], W]];
    steps.iter().map(|cols| {
        let mut rgba = img.rgba.clone();
        for px in rgba.chunks_mut(4) {
            for (k, idx) in [2usize, 10, 12].iter().enumerate() {
                if px[..3] == assets::EGA[*idx] { px[..3].copy_from_slice(&cols[k]); break; }
            }
        }
        tex(&assets::Image { w: img.w, h: img.h, rgba })
    }).collect()
}

/// Read a sprite through one of the original's pointer tables (DS-relative).
fn table_sprite(exe: &exe::Exe, table: usize, idx: usize, w: usize, h: usize) -> Texture2D {
    let p = exe.ds(table + idx * 2, 2);
    let ptr = u16::from_le_bytes([p[0], p[1]]) as usize;
    let plane = (w / 8) * h;
    let b = exe.ds(ptr, plane * 5);
    tex(&assets::planes_to_rgba_masked(&b[..plane * 4], &b[plane * 4..], w, h))
}

/// 16x32 sprite stored directly: 4 planes (256B) + mask (64B).
fn tall_sprite(exe: &exe::Exe, off: usize) -> Texture2D {
    let b = exe.ds(off, 320);
    tex(&assets::planes_to_rgba_masked(&b[..256], &b[256..], 16, 32))
}

fn load_sprites(exe: &exe::Exe, dir: &PathBuf) -> Sprites {
    let items = [0usize, 1].map(|fr| {
        (0..16).map(|t| {
            let b = exe.ds(0x9e84 + fr * 0xa00 + t * 160, 160);
            tex(&assets::planes_to_rgba_masked(&b[..128], &b[128..], 16, 16))
        }).collect()
    });
    let screens: Vec<assets::Image> = (0..6).map(|n| assets::load_ega(&dir.join(format!("sys00{n}.ega"))).expect("ega screen")).collect();
    let opaque = |off: usize, w: usize, h: usize| tex(&assets::planes_to_rgba(exe.ds(off, w / 8 * h * 4), None, w, h));
    // like `opaque`, but pixels of EGA colour `key` become transparent
    let keyed = |off: usize, w: usize, h: usize, key: &[usize]| {
        let mut img = assets::planes_to_rgba(exe.ds(off, w / 8 * h * 4), None, w, h);
        for px in img.rgba.chunks_mut(4) { if key.iter().any(|&k| px[..3] == assets::EGA[k]) { px[3] = 0; } }
        tex(&img)
    };
    Sprites {
        player: (0..10).map(|f| tall_sprite(exe, 0x70c0 + f * 320)).collect(),
        items,
        fireball: (0..2).map(|i| table_sprite(exe, 0x9e60, i, 16, 8)).collect(),
        explosion: (0..5).map(|i| table_sprite(exe, 0x9e64, i, 16, 16)).collect(),
        puff: (0..5).map(|i| table_sprite(exe, 0x9e7a, i, 16, 32)).collect(),
        beam: (0..12).map(|k| tall_sprite(exe, 0x8f60 + k * 320)).collect(),
        death: (0..8).map(|k| tall_sprite(exe, 0x81a0 + k * 320)).collect(),
        digits: (0..10).map(|d| opaque(0xb284 + d * 64, 8, 16)).collect(),
        pips: (0..3).map(|i| opaque(0xb504 + i * 64, 8, 16)).collect(),
        life: [opaque(0xa7e4, 16, 16), opaque(0xa7e4 + 0xa00, 16, 16)],
        ov_digits: (0..10).map(|d| keyed(0xb284 + d * 64, 8, 16, &[0])).collect(),
        ov_pips: (0..3).map(|i| keyed(0xb504 + i * 64, 8, 16, &[0, 7, 15])).collect(),
        ov_life: [keyed(0xa7e4, 16, 16, &[7]), keyed(0xa7e4 + 0xa00, 16, 16, &[7])],
        banner: opaque(0xc1c4, 128, 48),
        screens: screens.iter().map(tex).collect(),
        frame_wide: tex(&widen_frame(&screens[3], WIDE_EXTRA)),
        fades: [fade_steps(&screens[0]), fade_steps(&screens[1]), fade_steps(&screens[2])],
        pause: opaque(0xb5c4, 128, 48),
        hint: tex(&assets::Image { w: 1, h: 1, rgba: vec![0, 0, 0, 255] }),
        hiscore_bg: assets::Image { w: screens[5].w, h: screens[5].h, rgba: screens[5].rgba.clone() },
    }
}

/// Per level: tile textures, and for each sprite entry the 10 frame slots the original uses
/// (0..4 = frames for one direction, 5..9 = the other).
struct LevelGfx {
    tiles: Vec<Texture2D>,
    enemies: Vec<Vec<Option<Texture2D>>>,
    /// The base's window and console pixels (Effects).
    spots: Option<Vec<fx::Spots>>,
}

fn load_level_gfx(dir: &PathBuf, level: &data::Level) -> LevelGfx {
    let tileset = assets::load_tileset(&dir.join(&level.tileset)).expect("tileset");
    let tiles = tileset.tiles.iter().map(tex).collect();
    let spots = ["base.tt2", "shed.tt2", "comp.tt2"].iter().any(|n| level.tileset.eq_ignore_ascii_case(n)).then(|| tileset.tiles.iter().map(|img| {
        let mut sp = fx::Spots::default();
        for (i, px) in img.rgba.chunks(4).enumerate() {
            let (x, y) = ((i % img.w) as u8, (i / img.w) as u8);
            let rgb = [px[0], px[1], px[2]];
            if rgb == assets::EGA[0] { sp.black.push((x, y)); }
            else if [4, 12, 2, 10, 14].iter().any(|&c| rgb == assets::EGA[c]) { sp.lights.push((x, y, [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0])); }
        }
        sp
    }).collect());
    let enemies = level.sprites.iter().map(|def| {
        let frames = assets::load_sprite(&dir.join(&def.name)).unwrap_or_default();
        let n = def.frames;
        let mut slots: Vec<Option<Texture2D>> = (0..10).map(|_| None).collect();
        let build = |start: usize| -> Vec<Option<Texture2D>> {
            let mut v: Vec<Option<Texture2D>> = (0..n).map(|i| frames.get(start + i).map(tex)).collect();
            if def.extra > 0 && n >= 2 { v.push(frames.get(start + n - 2).map(tex)); }
            v
        };
        let a = build(0);
        let b = if def.mode == 2 { build(n) } else { build(0) };
        for (i, t) in a.into_iter().enumerate().take(5) { slots[i] = t; }
        for (i, t) in b.into_iter().enumerate().take(5) { slots[5 + i] = t; }
        slots
    }).collect();
    LevelGfx { tiles, enemies, spots }
}

/// Draw a texture at virtual-screen coordinates (320x200 space).
fn blit(t: &Texture2D, f: &Frame, x: f32, y: f32) {
    let (sx, sy, w, h) = f.px(x, y, t.width(), t.height());
    draw_texture_ex(t, sx, sy, WHITE, DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
}

/// Like `blit_unit`, with an opacity.
fn blit_unit_a(t: &Texture2D, f: &Frame, ux: f32, uy: f32, cam: f32, alpha: f32) {
    let (sx, sy, w, h) = f.px(8.0 + f.wx + (ux - cam) * 8.0, 8.0 + uy * 8.0, t.width(), t.height());
    draw_texture_ex(t, sx, sy, Color::new(1.0, 1.0, 1.0, alpha), DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
}

/// Draw a texture at playfield unit coordinates (1 unit = 8px), relative to the camera.
fn blit_unit(t: &Texture2D, f: &Frame, ux: f32, uy: f32, cam: f32) {
    blit(t, f, 8.0 + f.wx + (ux - cam) * 8.0, 8.0 + uy * 8.0);
}

/// Where the 320x200 virtual screen sits in the window. `s` is the horizontal scale, `sy` the vertical one:
/// DOS showed 320x200 on a 4:3 monitor, so pixels were 1.2x taller than wide; the picture is always shown that way.
/// `ex` is the extra width (virtual px) the widescreen layout adds to the 320-wide screen, `wx` the part of it to the
/// left of the original playfield.
#[derive(Clone, Copy)]
struct Frame { x: f32, y: f32, s: f32, sy: f32, ex: f32, wx: f32 }

impl Frame {
    /// A virtual-screen rectangle in window pixels, with edges snapped so neighbouring tiles never leave seams.
    fn px(&self, x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
        let (x0, x1) = ((self.x + x * self.s).round(), (self.x + (x + w) * self.s).round());
        let (y0, y1) = ((self.y + y * self.sy).round(), (self.y + (y + h) * self.sy).round());
        (x0, y0, x1 - x0, y1 - y0)
    }
}

/// Test hook: CC_HOLD=left,jump,... holds those actions down.
fn env_hold() -> Vec<String> {
    std::env::var("CC_HOLD").map(|v| v.split(',').map(|x| x.trim().to_string()).collect()).unwrap_or_default()
}

fn apply_hold(i: &mut Input, hold: &[String]) {
    for h in hold {
        match h.as_str() { "left" => i.left = true, "right" => i.right = true, "jump" => i.jump = true,
            "open" => i.open = true, "fire" => i.fire = true, "tele" => i.teleport = true, _ => {} }
    }
}

/// Held-key state for the six actions: the configured DOS-scancode keys plus convenient aliases
/// (WASD / arrows / Enter / F / Z / T / X), unless an alias key is bound to a different action.
fn read_input(b: &input::Bindings) -> Input {
    let bound = b.to_bytes();
    let alias = |k: KeyCode, action_sc: u8| is_key_down(k) && input::scancode(k).map_or(true, |sc| sc == action_sc || !bound.contains(&sc));
    Input {
        left: input::is_down(b.left) || alias(KeyCode::A, b.left),
        right: input::is_down(b.right) || alias(KeyCode::D, b.right),
        jump: input::is_down(b.jump) || alias(KeyCode::W, b.jump) || alias(KeyCode::Up, b.jump),
        open: input::is_down(b.open) || alias(KeyCode::S, b.open) || alias(KeyCode::Down, b.open) || alias(KeyCode::Enter, b.open),
        fire: input::is_down(b.fire) || alias(KeyCode::F, b.fire) || alias(KeyCode::Z, b.fire) || alias(KeyCode::LeftControl, b.fire),
        teleport: input::is_down(b.teleport) || alias(KeyCode::T, b.teleport) || alias(KeyCode::X, b.teleport),
    }
}

/// Plays sounds. Priority between game sounds is decided by the game itself (see Game::play_fields), so a new
/// sound simply replaces whatever is playing.
struct Audio {
    sounds: HashMap<Sound, macroquad::audio::Sound>,
    current: Option<Sound>,
    muted: bool,
    /// 0.0..=0.7 (0.35 is the original loudness)
    volume: f32,
}

impl Audio {
    async fn load(exe: &exe::Exe) -> Audio {
        let mut sounds = HashMap::new();
        if std::env::var("CC_NO_AUDIO").is_ok() { return Audio { sounds, current: None, muted: true, volume: 0.35 }; }
        for s in sound::ALL {
            let (wav, _secs) = sound::synth(&sound::sequence(exe, s), 22050);
            if let Ok(snd) = load_sound_from_bytes(&wav).await { sounds.insert(s, snd); }
        }
        Audio { sounds, current: None, muted: false, volume: 0.35 }
    }

    fn play(&mut self, s: Sound) {
        if self.muted { return; }
        if let Some(cur) = self.current.and_then(|c| self.sounds.get(&c)) { stop_sound(cur); }
        if let Some(snd) = self.sounds.get(&s) {
            play_sound(snd, PlaySoundParams { looped: false, volume: self.volume });
            self.current = Some(s);
        }
    }

    fn stop_all(&mut self) {
        for snd in self.sounds.values() { stop_sound(snd); }
        self.current = None;
    }
}

/// Minimal 80x25 text terminal (enough for the original's INT 10h teletype / INT 21h print output).
struct Term { cells: Vec<Vec<char>>, row: usize, col: usize }

impl Term {
    fn new() -> Term { Term { cells: vec![vec![' '; 80]; 25], row: 0, col: 0 } }
    fn put(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '\r' => self.col = 0,
                '\n' => self.row += 1,
                '\x07' | '\x1a' => {}
                c => { if self.row < 25 { self.cells[self.row][self.col] = c; } self.col += 1; if self.col >= 80 { self.col = 0; self.row += 1; } }
            }
            if self.row >= 25 { self.cells.remove(0); self.cells.push(vec![' '; 80]); self.row = 24; }
        }
    }
}

/// The shareware notice: DS:0x17a, every byte XOR 0x25, terminated by Ctrl-Z.
fn notice_text(exe: &exe::Exe) -> String {
    exe.ds(0x17a, 1500).iter().map(|b| b ^ 0x25).take_while(|&c| c != 0x1a).map(|c| c as char).collect()
}

/// A '$'-terminated DOS string from the key-definition segment (0x1fba).
fn dos_str(exe: &exe::Exe, off: usize, max: usize) -> String {
    exe.seg(0x1fba, off, max).iter().take_while(|&&c| c != b'$').map(|&c| c as char).collect()
}

fn key_name(exe: &exe::Exe, sc: u8) -> String { dos_str(exe, 0x404a + 8 * (sc as usize - 1), 8) }

/// "Define Keys" dialog (original 0x008a): six prompts, then "setup OK?" and "save?".
#[derive(Default)]
struct KeyConfig { got: Vec<u8>, stage: u8 } // stage 0 prompting, 1 "OK?", 2 "save?"

const PROMPTS: [usize; 6] = [0x3eb9, 0x3ee9, 0x3f19, 0x3f49, 0x3f79, 0x3fa9]; // left, right, jump, fire, open, teleport

impl KeyConfig {
    fn render(&self, exe: &exe::Exe) -> Term {
        let mut t = Term::new();
        t.put(&dos_str(exe, 0x3e82, 80));
        for (i, off) in PROMPTS.iter().enumerate() {
            if i > self.got.len() { break; }
            t.put(&dos_str(exe, *off, 80));
            if let Some(&sc) = self.got.get(i) { t.put(&key_name(exe, sc)); }
        }
        if self.stage >= 1 { t.put(&dos_str(exe, 0x3fd9, 80)); }
        if self.stage >= 2 { t.put(&dos_str(exe, 0x4011, 80)); }
        t
    }

    fn bindings(&self) -> input::Bindings {
        let g = &self.got;
        input::Bindings { left: g[0], right: g[1], jump: g[2], fire: g[3], open: g[4], teleport: g[5] }
    }

    /// Feed a key press. Returns Some(save?) when the dialog is finished.
    fn key(&mut self, k: KeyCode) -> Option<bool> {
        match self.stage {
            0 => {
                if let Some(sc) = input::scancode(k) {
                    if input::assignable(sc) && !self.got.contains(&sc) { self.got.push(sc); }
                    if self.got.len() == 6 { self.stage = 1; }
                }
                None
            }
            1 => { match k { KeyCode::Y => self.stage = 2, KeyCode::N => { self.got.clear(); self.stage = 0; } _ => {} } None }
            _ => Some(k == KeyCode::Y),
        }
    }
}

fn draw_term(t: &Term, font: Option<&Font>) {
    let s = (screen_width() / 640.0).min((screen_height() - 30.0) / 400.0);
    let (ox, oy) = ((screen_width() - 640.0 * s) / 2.0, 8.0);
    draw_rectangle(ox, oy, 640.0 * s, 400.0 * s, BLACK);
    let size = (16.0 * s) as u16;
    for (r, row) in t.cells.iter().enumerate() {
        for (c, ch) in row.iter().enumerate() {
            if *ch == ' ' { continue; }
            draw_text_ex(&ch.to_string(), ox + c as f32 * 8.0 * s, oy + (r as f32 * 16.0 + 12.5) * s,
                TextParams { font, font_size: size, color: Color::from_rgba(170, 170, 170, 255), ..Default::default() });
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Screen { Notice, Keys, Title(u32), Story(u32), Legend, Prepare(u32), Play, HighScore }

/// The high-score screen (original 0x2190): optional name entry, then the table, then a key ends the program.
/// Letters offered when entering a name with a controller.
const PAD_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .-";

/// One frame of input for the name entry, from the keyboard and the controller at the same time.
#[derive(Default, Clone)]
struct NameInput { chars: Vec<char>, backspace: bool, enter: bool, up: bool, down: bool, add: bool, del: bool, start: bool }

impl NameInput {
    /// Typed characters and keys, plus the controller: d-pad up/down picks a letter, A or right adds it, B or left
    /// deletes, Start finishes. The arrow keys mirror the d-pad.
    fn read(pd: &pad::Pressed) -> NameInput {
        let mut chars = vec![];
        while let Some(c) = get_char_pressed() { chars.push(c); }
        NameInput {
            chars,
            backspace: is_key_pressed(KeyCode::Backspace),
            enter: is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter),
            up: pd.up || is_key_pressed(KeyCode::Up),
            down: pd.down || is_key_pressed(KeyCode::Down),
            add: pd.ok || pd.right || is_key_pressed(KeyCode::Right),
            del: pd.back || pd.left || is_key_pressed(KeyCode::Left),
            start: pd.start,
        }
    }
}

struct HighScore {
    ts: hiscore::TextScreen, table: Vec<u8>, entry: Option<usize>, name: Vec<u8>,
    /// the letter being picked with the controller (an index into PAD_CHARS), shown at the cursor
    cand: Option<usize>,
    /// the picked letter was changed since the last add, so finishing includes it
    pending: bool,
    dirty: bool,
    tex: Option<Texture2D>,
}

impl HighScore {
    fn new(bg: &assets::Image, font: Option<&hiscore::Font>, path: &std::path::Path, score: u32, pad_hint: bool) -> HighScore {
        let mut ts = hiscore::TextScreen::new(bg);
        let mut table = hiscore::load(path);
        let sc = hiscore::score_bytes(score);
        let entry = hiscore::rank(&table, sc);
        if let Some(i) = entry {
            hiscore::insert(&mut table, i, sc);
            ts.cursor(10, 14); ts.print(font, b"Great Score!$", 9);
            ts.cursor(11, 5); ts.print(font, b"Enter Your Name: $", 10);
            if pad_hint {
                ts.cursor(14, 5); ts.print(font, b"Pad: up/down = letter, A = add$", 8);
                ts.cursor(15, 5); ts.print(font, b"B = delete, Start = finish$", 8);
            }
            ts.cursor(11, 5 + 17); // the name goes right after the prompt (printing the hint moved the cursor away)
        } else {
            ts.draw_table(font, &table);
        }
        HighScore { ts, table, entry, name: vec![], cand: None, pending: false, dirty: true, tex: None }
    }

    /// The screen as a texture (rebuilt only after a change).
    fn texture(&mut self) -> &Texture2D {
        if self.dirty || self.tex.is_none() { self.tex = Some(tex(&self.ts.img)); self.dirty = false; }
        self.tex.as_ref().unwrap()
    }

    /// Show (or, with `None`, erase) the letter being picked at the cursor without moving the cursor.
    fn preview(&mut self, font: Option<&hiscore::Font>, c: Option<usize>) {
        if self.name.len() >= hiscore::NAME_MAX { return; }
        self.ts.teletype(font, c.map_or(b' ', |i| PAD_CHARS[i]), 14);
        self.ts.teletype(font, 8, 7);
    }

    /// DOS buffered input (INT 21h AH=0Ah, 12 characters) echoed in grey; Enter stores the name, saves the file and
    /// shows the table. The keyboard and the controller work at the same time and on the same name: typed letters are
    /// added as they come, the controller picks a letter (shown in yellow at the cursor) and adds it. Every change goes
    /// through here in a fixed order and the picked letter is erased before and redrawn after, so mixing the two never
    /// leaves a stray letter on the screen. Returns true once the table is up.
    fn input(&mut self, font: Option<&hiscore::Font>, path: &std::path::Path, ev: &NameInput) -> bool {
        let Some(i) = self.entry else { return true };
        self.preview(font, None); // take the picked letter off the screen while the name changes
        // 1. typed characters
        for &c in &ev.chars {
            if (' '..='~').contains(&c) && self.name.len() < hiscore::NAME_MAX {
                self.name.push(c as u8); self.ts.teletype(font, c as u8, 7);
                self.cand = None; self.pending = false;
            }
        }
        // 2. deleting (keyboard Backspace, pad B / left)
        if ev.backspace || ev.del {
            if self.name.pop().is_some() { self.ts.teletype(font, 8, 7); self.ts.teletype(font, b' ', 7); self.ts.teletype(font, 8, 7); }
        }
        // 3. picking a letter
        let step: i32 = if ev.up { 1 } else if ev.down { -1 } else { 0 };
        if step != 0 && self.name.len() < hiscore::NAME_MAX {
            let n = PAD_CHARS.len() as i32;
            self.cand = Some(self.cand.map_or(if step > 0 { 0 } else { n as usize - 1 }, |c| (c as i32 + step).rem_euclid(n) as usize));
            self.pending = true;
        }
        // 4. adding the picked letter; the same letter stays up for the next position
        if ev.add && self.name.len() < hiscore::NAME_MAX {
            if let Some(c) = self.cand { self.name.push(PAD_CHARS[c]); self.ts.teletype(font, PAD_CHARS[c], 7); self.pending = false; }
        }
        // 5. finishing: a picked letter that was not added yet still counts
        if ev.enter || ev.start {
            if let (Some(c), true, true) = (self.cand, self.pending, self.name.len() < hiscore::NAME_MAX) { self.name.push(PAD_CHARS[c]); }
            hiscore::set_name(&mut self.table, i, &self.name);
            let blank = [b' '; 30];
            for row in [10, 11, 14, 15] { self.ts.cursor(row, 5); self.ts.print(font, &blank, 1); }
            let _ = std::fs::write(path, &self.table);
            self.ts.draw_table(font, &self.table);
            self.entry = None;
        } else {
            self.preview(font, self.cand);
        }
        self.dirty = true;
        false
    }
}

fn is_modifier(k: KeyCode) -> bool {
    matches!(k, KeyCode::LeftShift | KeyCode::RightShift | KeyCode::LeftControl | KeyCode::RightControl | KeyCode::LeftAlt
        | KeyCode::RightAlt | KeyCode::CapsLock | KeyCode::NumLock | KeyCode::ScrollLock | KeyCode::LeftSuper | KeyCode::RightSuper)
}

/// Title: fades in over 4 ticks, then holds; the original never reads the keyboard here (0x0209..0x0232).
const TITLE_TICKS: u32 = 15;
/// Pre-game screen (0x02e2): the bare game frame, a life icon lights up each tick (5), 3 ticks' pause, one life is
/// spent and the level starts.
const PREPARE_TICKS: u32 = 8;

#[macroquad::main(window_conf)]
async fn main() {
    // Find the original game: a folder or zip given on the command line (unquoted paths with spaces are fine),
    // otherwise the `game` folder in the project root where the zip is dropped.
    let mut given: Vec<String> = std::env::args().skip(1).collect();
    // Settings, saves, key bindings and high scores live in files next to the exe, wherever it is started from.
    // (Paths given on the command line are made absolute first.)
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        for g in given.iter_mut() { if let Ok(a) = std::fs::canonicalize(&*g) { *g = a.to_string_lossy().trim_start_matches(r"\\?\").to_string(); } }
        let _ = std::env::set_current_dir(exe_dir);
    }
    let (dir, exe) = match gamefiles::locate(&given) {
        Ok(x) => x,
        Err(mut msg) => {
            msg.push(String::new());
            msg.push("Press any key to close.".to_string());
            eprintln!("{}", msg.join(" | "));
            loop {
                clear_background(Color::new(0.05, 0.05, 0.08, 1.0));
                for (i, l) in msg.iter().enumerate() { draw_text(l, 30.0, 60.0 + i as f32 * 28.0, 24.0, if i == 0 { RED } else { WHITE }); }
                if get_last_key_pressed().is_some() { return; }
                next_frame().await;
            }
        }
    };
    let mut sprites = load_sprites(&exe, &dir);
    let levels: Vec<data::Level> = (0..data::LEVEL_COUNT).map(|i| data::level(&exe, i)).collect();
    let gfx: Vec<LevelGfx> = levels.iter().map(|l| load_level_gfx(&dir, l)).collect();
    let maps: Vec<Vec<Map>> = levels.iter().map(|l| {
        let solid_above = assets::load_tileset(&dir.join(&l.tileset)).expect("tileset").header[0];
        l.maps.iter().map(|m| {
            let m = assets::load_map(&dir.join(m)).expect("map");
            Map { cells: m.cells, w: m.w, h: m.h, solid_above }
        }).collect()
    }).collect();

    let mut audio = Audio::load(&exe).await;
    let lens = sound::ALL.iter().map(|&s| (s, sound::total_ticks(&exe, s))).collect();
    let mut game = Game::new(levels, maps, lens);
    let hold: Vec<String> = env_hold();
    let env = |k: &str| std::env::var(k).ok();
    if let Some(l) = env("CC_LEVEL").and_then(|v| v.parse::<usize>().ok()) { game.debug_goto(l % data::LEVEL_COUNT); }
    if env("CC_CHEAT").is_some() { game.max_fireballs = 5; game.items.key = true; game.items.corkscrew = true; game.items.wand = true; game.items.boots = true; game.items.lantern = true; }
    if env("CC_GAMEOVER").is_some() { game.phase = Phase::GameOver; game.game_over = true; }
    if env("CC_ENDING").is_some() { game.phase = Phase::Ending(Ending::BeamOut(0)); }
    // Deterministic render for the pixel comparison: run a tick script (same format as the trace tool), then freeze.
    let mut frozen = false;
    if let Some(path) = env("CC_RENDER") {
        while game.phase != Phase::Playing { game.tick(Input::default()); }
        if env("CC_LIVES5").is_some() { game.lives = 5; }
        if env("CC_CHEAT").is_some() { game.player.reload_jump(5); }
        for l in std::fs::read_to_string(&path).unwrap_or_default().lines() {
            let mut it = l.split_whitespace();
            let (Some(n), Some(keys)) = (it.next(), it.next()) else { continue };
            let n: u32 = n.parse().unwrap_or(0);
            let has = |k: &str| keys.split(',').any(|x| x == k);
            let inp = Input { left: has("left"), right: has("right"), jump: has("jump"), open: has("open"), fire: has("fire"), teleport: has("tele") };
            for _ in 0..n {
                game.tick(inp);
                while game.dying.is_some() || matches!(game.phase, Phase::Door(_) | Phase::Fall(_)) { game.tick(Input::default()); }
            }
        }
        frozen = true;
    }
    let mut screen = match env("CC_SCREEN").as_deref() {
        Some("keys") => Screen::Keys, Some("title") => Screen::Title(0), Some("story") => Screen::Story(0), Some("legend") => Screen::Legend, Some("prepare") => Screen::Prepare(0), Some("play") => Screen::Play,
        _ if frozen => Screen::Play,
        _ if env("CC_LEVEL").is_some() || env("CC_ENDING").is_some() || env("CC_GAMEOVER").is_some() || env("CC_CHEAT").is_some() => Screen::Play,
        _ => Screen::Notice,
    };
    let font = load_ttf_font("C:/Windows/Fonts/consola.ttf").await.ok();
    let notice = notice_text(&exe);
    let mut bindings = input::Bindings::load(&dir);
    let mut kc = KeyConfig::default();
    let speed: f32 = env("CC_FAST").and_then(|v| v.parse().ok()).unwrap_or(1.0);
    // The original changes the screen only on a game tick (every 2 timer interrupts, ~9.1 Hz) and moves things in
    // whole 8-pixel steps; the host does the same: no interpolation, ticks paced by the wall clock.
    let mut acc = 0.0f64;
    let mut last_time = get_time();
    let mut host_acc = 0.0f64;
    let mut title_started = false;
    // Esc pause (original 0x1383 / 0x0fdf): Some(false) waits for a key (Q quits), Some(true) for Esc to be released
    let mut pause: Option<bool> = None;
    let bios_font = hiscore::Font::load();
    {
        let mut ts = hiscore::TextScreen::new(&assets::Image { w: 168, h: 8, rgba: [0, 0, 0, 255].repeat(168 * 8) });
        ts.print(bios_font.as_ref(), b"You need the Door Key$", 14);
        sprites.hint = tex(&ts.img);
    }
    let hgh_path = PathBuf::from("COMIC.HGH");
    let mut hs: Option<HighScore> = None;
    if env("CC_SCREEN").as_deref() == Some("hiscore") {
        let score = env("CC_SCORE").and_then(|v| v.parse().ok()).unwrap_or(0);
        hs = Some(HighScore::new(&sprites.hiscore_bg, bios_font.as_ref(), &hgh_path, score, true));
        screen = Screen::HighScore;
    }
    if env("CC_PAUSE").is_some() { pause = Some(false); }
    let mut cfg = settings::Settings::load();
    if env("CC_WIDE").is_some() { cfg.widescreen = true; }
    if env("CC_CRT").is_some() { cfg.crt = true; }
    if env("CC_INFFIRE").is_some() { cfg.cheat_fire = true; }
    let force_square = env("CC_SQUARE").is_some() || env("CC_RENDER").is_some();
    let mut windowed = (screen_width(), screen_height());
    if cfg.fullscreen { apply_fullscreen(true, &mut windowed); }
    let mut menu: Option<menu::Menu> = env("CC_MENU").map(|_| menu::Menu::default());
    let mut pad = pad::Pad::new();
    let mut toast: Option<(String, f64)> = None;
    let mut crt_rt: Option<((u32, u32), RenderTarget)> = None;
    let lighting = light::Lighting::new();
    let crt_mat = load_material(ShaderSource::Glsl { vertex: CRT_VERTEX, fragment: CRT_FRAGMENT },
        MaterialParams { uniforms: vec![UniformDesc::new("TexW", UniformType::Float1), UniformDesc::new("Lines", UniformType::Float1)], ..Default::default() }).expect("crt shader");
    let mut esc_wait = false;
    let mut view_extra: i32 = 0; // extra playfield units each side that the last frame showed (fireball range)
    let mut snap = Snap::of(&game);
    let mut fx = fx::Fx::new();
    // smooth motion: where each enemy is drawn; it glides toward its real position every frame
    let mut vis: Vec<(f32, f32)> = vec![];
    let mut vis_key = (usize::MAX, usize::MAX, u32::MAX);
    if env("CC_EFFECTS").is_some() { cfg.effects = true; }
    if env("CC_IMMERSIVE").is_some() { cfg.immersive = true; }
    let save_path = "comic.sav";
    let mut frames = 0u64;
    let mut last_door: Option<(bool, u32)> = None;
    let mut last_fall: Option<u32> = None;
    let mut door_shot_no = 0;

    loop {
        let now = get_time();
        let dt = ((now - last_time).min(0.25) * speed as f64) as f32;
        last_time = now;
        let last_key = get_last_key_pressed();
        let any_key = last_key.is_some() || is_mouse_button_pressed(MouseButton::Left);

        audio.volume = cfg.volume as f32 * 0.07;
        if is_key_pressed(KeyCode::F2) { audio.muted = !audio.muted; if audio.muted { audio.stop_all(); } }
        if is_key_pressed(KeyCode::F11) { cfg.fullscreen = !cfg.fullscreen; apply_fullscreen(cfg.fullscreen, &mut windowed); cfg.save(); }
        let pd = pad.poll();
        if pd.select && !menu.as_ref().map_or(false, |m| m.pad_remap.is_some()) {
            let on = cfg.enhanced_count() < settings::ENHANCED_TOTAL;
            cfg.set_enhanced(on);
            cfg.save();
            toast = Some((format!("Enhanced {}", if on { "on" } else { "off" }), get_time() + 2.0));
        }

        // settings menu (F1): takes over the keyboard while open
        let was_open = menu.is_some();
        if !was_open && (is_key_pressed(KeyCode::F1) || pd.start) && matches!(screen, Screen::Title(_) | Screen::Story(_) | Screen::Legend | Screen::Play) {
            menu = Some(menu::Menu::default());
        }
        let in_menu = menu.is_some();
        if let Some(m) = menu.as_mut().filter(|_| was_open) {
            let mut b = bindings;
            let was_fullscreen = cfg.fullscreen;
            let mut outcome = menu::Outcome::None;
            if m.pad_remap.is_some() {
                if last_key == Some(KeyCode::Escape) || pd.start { m.pad_remap = None; }
                else if let Some(btn) = pd.button {
                    let mut map = cfg.pad;
                    let done = m.pad_button(btn, &mut map);
                    cfg.pad = map;
                    if done { cfg.save(); toast = Some(("Controller saved".into(), get_time() + 2.0)); }
                }
            } else if m.remap.is_some() {
                if last_key == Some(KeyCode::Escape) || pd.back { m.nav(menu::Nav::Back, &mut cfg, &mut b); }
                else if let Some(k) = last_key { if m.remap_key(input::scancode(k), &mut b) { bindings = b; let _ = bindings.save(); toast = Some(("Keys saved".into(), get_time() + 2.0)); } else { bindings = b; } }
            } else {
                let navs = [(KeyCode::Up, pd.up, menu::Nav::Up), (KeyCode::Down, pd.down, menu::Nav::Down), (KeyCode::Left, pd.left, menu::Nav::Left),
                    (KeyCode::Right, pd.right, menu::Nav::Right), (KeyCode::Enter, pd.ok, menu::Nav::Ok), (KeyCode::Escape, pd.back, menu::Nav::Back),
                    (KeyCode::F1, pd.start, menu::Nav::Back), (KeyCode::Backspace, false, menu::Nav::Back)];
                for (k, pad_hit, n) in navs {
                    if is_key_pressed(k) || pad_hit { outcome = m.nav(n, &mut cfg, &mut b); if outcome != menu::Outcome::None { break; } }
                }
            }
            if outcome == menu::Outcome::Changed {
                cfg.save();
                if cfg.fullscreen != was_fullscreen { apply_fullscreen(cfg.fullscreen, &mut windowed); }
            }
            if outcome == menu::Outcome::Quit { return; }
            if outcome == menu::Outcome::Save {
                let msg = if screen != Screen::Play || !game.can_save() { "Can't save right now" } else if std::fs::write(save_path, game.save_text()).is_ok() { "Game saved" } else { "Save failed" };
                toast = Some((msg.into(), get_time() + 2.0));
                if msg == "Game saved" { menu = None; esc_wait = true; }
            }
            if outcome == menu::Outcome::Load {
                let ok = std::fs::read_to_string(save_path).map(|t| game.load_text(&t)).unwrap_or(false);
                if ok {
                    audio.stop_all(); snap = Snap::of(&game); fx.reset(game.score);
                    pause = None; acc = 0.0; screen = Screen::Play; menu = None; esc_wait = true;
                }
                toast = Some((if ok { "Game loaded" } else { "No saved game" }.into(), get_time() + 2.0));
            }
            if outcome == menu::Outcome::Close { menu = None; esc_wait = true; }
            if outcome == menu::Outcome::Restart {
                menu = None;
                game.new_game();
                game.cheat_fire = cfg.cheat_fire;
                game.cheat_health = cfg.cheat_health;
                game.cheat_items = cfg.cheat_items;
                snap = Snap::of(&game);
                fx.reset(game.score);
                audio.stop_all();
                pause = None;
                esc_wait = true;
                acc = 0.0;
                screen = Screen::Play;
                toast = Some(("New game".into(), get_time() + 2.0));
            }
        }
        // while the menu is up (or was just opened / closed) the rest of the program sees no key presses
        let (last_key, any_key) = if in_menu { (None, false) } else { (last_key, any_key || pd.any) };

        // the title / story / pre-game screens are timed in game ticks like the original
        let mut host_ticks = 0;
        if matches!(screen, Screen::Title(_) | Screen::Story(_) | Screen::Prepare(_)) && menu.is_none() {
            host_acc += dt as f64;
            while host_acc >= TICK_SECS as f64 { host_acc -= TICK_SECS as f64; host_ticks += 1; }
        } else { host_acc = 0.0; }

        // ------------------------------------------------------------ screen flow
        match screen {
            Screen::Notice => {
                match last_key {
                    Some(KeyCode::K) => { kc = KeyConfig::default(); screen = Screen::Keys; }
                    Some(KeyCode::Escape) => return,
                    Some(_) => screen = Screen::Title(0),
                    None => if pd.any { screen = Screen::Title(0); }
                }
            }
            Screen::Keys => {
                if let Some(k) = last_key {
                    if let Some(save) = kc.key(k) {
                        bindings = kc.bindings();
                        if save { let _ = bindings.save(); }
                        screen = Screen::Title(0);
                    }
                }
            }
            Screen::Title(t) => {
                if t == 0 && !title_started { audio.play(Sound::Title); title_started = true; }
                let t = t + host_ticks;
                screen = if t >= TITLE_TICKS { Screen::Story(0) } else { Screen::Title(t) };
            }
            // keys pressed during the fade are thrown away (the original flushes the BIOS buffer before waiting)
            Screen::Story(t) => { let t = t + host_ticks; screen = if t > 4 && any_key { Screen::Legend } else { Screen::Story(t) }; }
            Screen::Legend => if any_key { screen = Screen::Prepare(0); audio.stop_all(); },
            Screen::Prepare(t) => {
                let t = t + host_ticks;
                if t >= PREPARE_TICKS { screen = Screen::Play; game.new_game(); fx.reset(0); acc = 0.0; } else { screen = Screen::Prepare(t); }
            }
            Screen::Play => {
                if is_key_pressed(KeyCode::Tab) && game.phase == Phase::Playing {
                    game.warp_to_level((game.level_idx + 1) % data::LEVEL_COUNT);
                }
                if pause.is_none() && (last_key.map_or(false, |k| !is_modifier(k)) || pd.any) && game.ending_key() {
                    // game over / the end: a key brings up the high-score table (0x104a -> 0x2190)
                    audio.stop_all();
                    while get_char_pressed().is_some() {}
                    hs = Some(HighScore::new(&sprites.hiscore_bg, bios_font.as_ref(), &hgh_path, game.score, pad.connected()));
                    screen = Screen::HighScore;
                }
                match pause {
                    Some(false) => {
                        if let Some(k) = last_key.filter(|k| !is_modifier(*k)) {
                            if k == KeyCode::Q { return; } // the original exits to DOS
                            pause = Some(true);
                        } else if pd.any { pause = Some(true); }
                    }
                    Some(true) => if !is_key_down(KeyCode::Escape) { pause = None; acc = 0.0; },
                    None => {}
                }
                // enhancement: quick save / load (F5 / F9)
                if pause.is_none() && menu.is_none() && game.phase == Phase::Playing {
                    if is_key_pressed(KeyCode::F5) {
                        let msg = if !game.can_save() { "Can't save right now" } else if std::fs::write(save_path, game.save_text()).is_ok() { "Game saved" } else { "Save failed" };
                        toast = Some((msg.into(), get_time() + 2.0));
                    }
                    if is_key_pressed(KeyCode::F9) {
                        let ok = std::fs::read_to_string(save_path).map(|t| game.load_text(&t)).unwrap_or(false);
                        if ok { audio.stop_all(); snap = Snap::of(&game); fx.reset(game.score); }
                        toast = Some((if ok { "Game loaded" } else { "No saved game" }.into(), get_time() + 2.0));
                    }
                }
                if esc_wait && !is_key_down(KeyCode::Escape) { esc_wait = false; }
                if frozen || pause.is_some() || menu.is_some() || screen != Screen::Play { acc = 0.0; } else { acc += dt as f64; }
                fx.weather = cfg.weather;
                if cfg.effects && pause.is_none() && menu.is_none() { fx.update(dt, &game); }
                if cfg.smooth && pause.is_none() && menu.is_none() {
                    let key = (game.level_idx, game.stage_idx, game.resets);
                    if key != vis_key || vis.len() != game.enemies.len() {
                        vis_key = key;
                        vis = game.enemies.iter().map(|e| (e.x as f32, e.y as f32)).collect();
                    }
                    let k = 1.0 - (-dt.min(0.1) * 14.0).exp();
                    for (v, e) in vis.iter_mut().zip(game.enemies.iter()) {
                        let (tx, ty) = (e.x as f32, e.y as f32);
                        // inactive enemies and big jumps (spawn, wrap-around) are not glided
                        if e.state != 1 || (tx - v.0).abs() > 8.0 || (ty - v.1).abs() > 8.0 { *v = (tx, ty); }
                        else { v.0 += (tx - v.0) * k; v.1 += (ty - v.1) * k; }
                    }
                }
                while acc >= TICK_SECS as f64 {
                    acc -= TICK_SECS as f64;
                    if game.phase == Phase::Playing && game.dying.is_none() && game.tele.is_none() && (is_key_down(KeyCode::Escape) && !esc_wait) {
                        pause = Some(false);
                        acc = 0.0;
                        break;
                    }
                    let mut inp = read_input(&bindings);
                    merge_input(&mut inp, pad.held(&cfg.pad));
                    apply_hold(&mut inp, &hold);
                    snap = Snap::of(&game);
                    game.cheat_fire = cfg.cheat_fire;
                    game.cheat_health = cfg.cheat_health;
                    game.cheat_items = cfg.cheat_items;
                    game.view_margin = view_extra;
                    game.tick(inp);
                    if cfg.rumble {
                        if game.hp < snap.hp { pad.rumble(40000, 220); }
                        if (game.dying.is_some() && !snap.dying) || (matches!(game.phase, Phase::Fall(_)) && !snap.fell) { pad.rumble(65000, 500); }
                    }
                    if cfg.effects { fx.smooth = cfg.smooth; fx.observe(&snap, &game); }
                }
                for s in game.sounds.drain(..) { audio.play(s); }
            }
            Screen::HighScore => {
                if let Some(h) = hs.as_mut() {
                    let typing = h.entry.is_some();
                    let shown = h.input(bios_font.as_ref(), &hgh_path, &NameInput::read(&pd));
                    // after the table: any key ends the program, as the original does (0x22fb, then exit to DOS)
                    if shown && !typing && (last_key.map_or(false, |k| !is_modifier(k)) || pd.any) { return; }
                }
            }
        }

        // ------------------------------------------------------------ layout (320x200 virtual screen)
        let ar = if force_square { 1.0 } else { 1.2 }; // DOS pixels were 1.2x taller than wide (CC_SQUARE is a test hook)
        let classic = |ex: f32| -> Frame {
            let cw = 320.0 + ex;
            let fit = (screen_width() / cw).min((screen_height() - 16.0) / (200.0 * ar) + 1e-3);
            let s = if cfg.scaling == settings::Scaling::Fit { fit.max(1.0) } else { fit.floor().max(1.0) };
            Frame { x: ((screen_width() - cw * s) / 2.0).floor(), y: ((screen_height() - 200.0 * s * ar) / 2.0).floor().max(0.0), s, sy: s * ar, ex, wx: ex / 2.0 }
        };
        let imm = cfg.immersive && screen == Screen::Play;
        let f = if imm {
            // the playfield (192 x 160 plus as much width as the window shows) fills the window; the frame's origin
            // (8, 8) maps to the top left corner
            let s = (screen_height() / (160.0 * ar)).min(screen_width() / 192.0);
            let ex = (screen_width() / s - 192.0).floor().max(0.0);
            Frame { x: (screen_width() - (192.0 + ex) * s) / 2.0 - 8.0 * s, y: (screen_height() - 160.0 * s * ar) / 2.0 - 8.0 * s * ar, s, sy: s * ar, ex, wx: ex / 2.0 }
        } else {
            classic(if cfg.widescreen && screen == Screen::Play { WIDE_EXTRA as f32 } else { 0.0 })
        };
        let cw = 320.0 + f.ex;
        view_extra = if imm { (f.ex / 16.0).ceil() as i32 } else if cfg.widescreen { WIDE_EXTRA as i32 / 16 } else { 0 };
        let fm = if imm { classic(0.0) } else { f }; // the menu is centred in the window as in the classic layout
        clear_background(Color::new(0.05, 0.05, 0.08, 1.0));

        // CRT filter: draw the picture into a 4x offscreen target, then put it on the screen through a shader
        let use_crt = cfg.crt && !matches!(screen, Screen::Notice | Screen::Keys);
        let rk = if cfg.crt { CRT_K } else { 1.0 };
        // the offscreen picture: the whole 320x200 screen, or only the playfield in the immersive layout
        let (vw, vh) = if imm { (192.0 + f.ex, 160.0) } else { (cw, 200.0) };
        let fr = if use_crt {
            let (tw, th) = ((vw * rk) as u32, (vh * rk) as u32);
            if crt_rt.as_ref().map_or(true, |(k, _)| *k != (tw, th)) {
                let rt = render_target(tw, th);
                rt.texture.set_filter(FilterMode::Linear);
                crt_rt = Some(((tw, th), rt));
            }
            let (_, rt) = crt_rt.as_ref().unwrap();
            let mut cam = Camera2D::from_display_rect(Rect::new(0.0, 0.0, vw * rk, vh * rk));
            cam.render_target = Some(rt.clone());
            set_camera(&cam);
            clear_background(BLACK);
            let o = if imm { -8.0 * rk } else { 0.0 };
            Frame { x: o, y: o, s: rk, sy: rk, ex: f.ex, wx: f.wx }
        } else { f };
        match screen {
            Screen::Notice => { let mut t = Term::new(); t.put(&notice); draw_term(&t, font.as_ref()); }
            Screen::Keys => draw_term(&kc.render(&exe), font.as_ref()),
            Screen::Title(t) => blit(sprites.fades[0].get(t as usize).unwrap_or(&sprites.screens[0]), &fr, 0.0, 0.0),
            Screen::Story(t) => blit(sprites.fades[1].get(t as usize).unwrap_or(&sprites.screens[1]), &fr, 0.0, 0.0),
            Screen::Prepare(t) => {
                blit(&sprites.screens[3], &fr, 0.0, 0.0);
                for i in 1..=t.min(5) { blit(&sprites.life[0], &fr, (3 + 3 * i) as f32 * 8.0, 180.0); }
            }
            Screen::Legend => blit(&sprites.screens[4], &fr, 0.0, 0.0),
            Screen::Play => {
                let a = (acc / TICK_SECS as f64).clamp(0.0, 1.0) as f32;
                let sm = (cfg.smooth && !frozen && pause.is_none()).then_some((&snap, a));
                let mut frs = fr;
                if cfg.effects { frs.x += fx.shake_off.0 * frs.s; frs.y += fx.shake_off.1 * frs.sy; }
                draw_play(&game, &frs, &sprites, &gfx, pause.is_some(), sm, (cfg.hints && game.hint > 0) || env("CC_SHOWHINT").is_some(), cfg.effects.then_some(&fx), (cfg.smooth && !frozen).then_some(vis.as_slice()), imm, [0.5, 0.75, 1.0][cfg.hud_size.min(2) as usize], cfg.lighting.then_some(&lighting));
            }
            Screen::HighScore => if let Some(h) = hs.as_mut() { blit(h.texture(), &fr, 0.0, 0.0) },
        }
        if use_crt {
            set_default_camera();
            let ((tw, _th), rt) = crt_rt.as_ref().unwrap();
            crt_mat.set_uniform("TexW", *tw as f32);
            crt_mat.set_uniform("Lines", vh);
            gl_use_material(&crt_mat);
            let r = if imm { f.px(8.0, 8.0, vw, vh) } else { f.px(0.0, 0.0, cw, 200.0) };
            draw_texture_ex(&rt.texture, r.0, r.1, WHITE, DrawTextureParams { dest_size: Some(vec2(r.2, r.3)), flip_y: CRT_FLIP, ..Default::default() });
            gl_use_default_material();
        }
        if let Some(m) = &menu { draw_menu(m, &cfg, &bindings, &Frame { x: fm.x + fm.ex / 2.0 * fm.s, ex: 0.0, wx: 0.0, ..fm }); }
        if let Some((msg, until)) = &toast { if get_time() < *until {
            let size = 36;
            draw_text_ex(msg, 14.0, screen_height() - 22.0, TextParams { font: font.as_ref(), font_size: size, color: Color::new(0.0, 0.0, 0.0, 0.85), ..Default::default() });
            draw_text_ex(msg, 12.0, screen_height() - 24.0, TextParams { font: font.as_ref(), font_size: size, color: Color::new(1.0, 1.0, 0.33, 1.0), ..Default::default() });
        } }

        if let (Ok(base), Phase::Fall(t)) = (std::env::var("CC_FALLSHOTS"), game.phase) {
            if last_fall != Some(t) {
                last_fall = Some(t);
                get_screen_data().export_png(&format!("{}_{:02}.png", base, t));
            }
        }
        if let (Ok(base), Phase::Door(d)) = (std::env::var("CC_DOORSHOTS"), game.phase) {
            if last_door != Some((d.arriving, d.frame)) {
                last_door = Some((d.arriving, d.frame));
                get_screen_data().export_png(&format!("{}_{:02}_{}{}.png", base, door_shot_no, if d.arriving { "in" } else { "out" }, d.frame));
                door_shot_no += 1;
            }
        }
        if let Ok(n) = std::env::var("CC_SHOT_AFTER").map(|v| v.parse::<u64>().unwrap_or(30)) {
            frames += 1;
            if frames == n {
                get_screen_data().export_png(&std::env::var("CC_SHOT_FILE").unwrap_or("shot.png".into()));
                return;
            }
        }
        next_frame().await;
    }
}

/// The previous tick's positions, for the optional smooth-motion mode (the game itself always runs at the original rate).
struct Snap { door: bool, air: bool, hp: i32, states: Vec<i32>, collected: usize, dying: bool, fell: bool, resets: u32, level: usize, stage: usize, y: i32, x: i32, cam: i32, enemies: Vec<(i32, i32)>, fbs: [Option<(i32, i32)>; 5] }

impl Snap {
    fn of(g: &Game) -> Snap {
        let mut fbs = [None; 5];
        for (i, f) in g.fireballs.iter().enumerate() { fbs[i] = f.map(|f| (f.y, f.x)); }
        Snap { door: matches!(g.phase, Phase::Door(_)), air: g.player.air, hp: g.hp, states: g.enemies.iter().map(|e| e.state).collect(), collected: g.collected.len(), dying: g.dying.is_some(), fell: matches!(g.phase, Phase::Fall(_)), resets: g.resets, level: g.level_idx, stage: g.stage_idx, y: g.player.y, x: g.player.x, cam: g.player.cam,
               enemies: g.enemies.iter().map(|e| (e.y, e.x)).collect(), fbs }
    }
}

/// Position between the previous and the current tick; big jumps (respawns, wraps) are not smoothed.
fn mix(prev: i32, cur: i32, a: f32) -> f32 {
    if (cur - prev).abs() > 8 { cur as f32 } else { prev as f32 + (cur - prev) as f32 * a }
}

/// Enter or leave fullscreen. Leaving restores the window size from before (set_fullscreen alone can leave it shrunk).
fn apply_fullscreen(on: bool, windowed: &mut (f32, f32)) {
    if on {
        *windowed = (screen_width(), screen_height());
        set_fullscreen(true);
    } else {
        set_fullscreen(false);
        request_new_screen_size(windowed.0, windowed.1);
    }
}

fn merge_input(i: &mut Input, o: Input) {
    i.left |= o.left; i.right |= o.right; i.jump |= o.jump; i.open |= o.open; i.fire |= o.fire; i.teleport |= o.teleport;
}

fn draw_menu(m: &menu::Menu, cfg: &settings::Settings, b: &input::Bindings, f: &Frame) {
    let lines = m.lines(cfg, b);
    let keys = menu::Menu::key_lines(b, &cfg.pad);
    let rows = lines.len().max(keys.len()) as f32;
    let (w, h) = (316.0 * f.s, (rows * 8.5 + 27.0) * f.s);
    let (x, y) = (f.x + (320.0 * f.s - w) / 2.0, f.y + (200.0 * f.sy - h) / 2.0);
    let yellow = Color::new(1.0, 1.0, 0.33, 1.0);
    let cyan = Color::new(0.33, 1.0, 1.0, 1.0);
    draw_rectangle(x, y, w, h, BLACK);
    draw_rectangle_lines(x, y, w, h, 2.0, yellow);
    let size = 8.0 * f.s * 1.35;
    draw_text(m.title(), x + 10.0 * f.s, y + 14.0 * f.s, size, yellow);
    let split = x + 184.0 * f.s;
    draw_line(split, y + 6.0 * f.s, split, y + h - 6.0 * f.s, 1.0, Color::new(0.5, 0.5, 0.2, 1.0));
    for (i, l) in lines.iter().enumerate() {
        let (label, value) = l.split_once('|').unwrap_or((l, ""));
        let ly = y + (24.0 + i as f32 * 8.5) * f.s;
        draw_text(label, x + 10.0 * f.s, ly, size * 0.75, WHITE);
        draw_text(value, x + 112.0 * f.s, ly, size * 0.75, cyan);
    }
    for (i, l) in keys.iter().enumerate() {
        let parts: Vec<&str> = l.split('|').collect();
        let (label, value, third) = (parts[0], parts.get(1).copied().unwrap_or(""), parts.get(2).copied().unwrap_or(""));
        if i == 0 { draw_text(label, split + 8.0 * f.s, y + 14.0 * f.s, size, yellow); draw_text(third, split + 100.0 * f.s, y + 14.0 * f.s, size * 0.75, yellow); continue; }
        let ly = y + (24.0 + (i - 1) as f32 * 8.5) * f.s;
        draw_text(label, split + 8.0 * f.s, ly, size * 0.65, WHITE);
        draw_text(value, split + 50.0 * f.s, ly, size * 0.65, cyan);
        draw_text(third, split + 100.0 * f.s, ly, size * 0.65, cyan);
    }
}

/// The lights of a level for the ray-traced lighting: (how much stays visible in the dark, the lights).
/// Every lit level has a light around Comic plus light-giving tiles: castle (only once the Lantern is found) and cave:
/// torches; base: console lights; shed: the coloured blocks; computer: the glowing discs and panels. Fireballs light
/// everything. The other levels (lake, forest, space) are outdoors and stay as they are.
fn scene_lights(g: &Game, lvl: usize, map: &Map, px: f32, py: f32, cam: f32, ex: f32) -> Option<(f32, Vec<light::Light>)> {
    // (ambient, Comic's light (radius, strength), [(tile ids, radius, strength)])
    let (ambient, comic, sources): (f32, (f32, f32), &[(&[u8], f32, f32)]) = match lvl {
        6 if g.items.lantern => (0.18, (125.0, 1.5), &[(&[60, 61], 64.0, 0.85)]),
        4 => (0.20, (110.0, 1.4), &[(&[6], 68.0, 0.85)]),
        3 => (0.72, (110.0, 1.0), &[(&[31, 34, 36, 37], 48.0, 0.7)]),
        5 => (0.68, (110.0, 1.0), &[(&[24, 28, 29, 30, 31], 48.0, 0.7)]),
        7 => (0.68, (110.0, 1.0), &[(&[41, 42, 43, 44, 45, 46, 47, 48, 49, 50], 56.0, 0.7), (&[2, 3, 4, 5], 48.0, 0.6)]),
        _ => return None,
    };
    let (cx0, cy0) = (px * 8.0 + 8.0, py * 8.0 + 16.0);
    let mut lights = vec![light::Light { x: cx0, y: cy0, radius: comic.0, strength: comic.1 }];
    for fb in g.fireballs.iter().flatten() {
        lights.push(light::Light { x: fb.x as f32 * 8.0 + 8.0, y: fb.y as f32 * 8.0 + 4.0, radius: 52.0, strength: 0.85 });
    }
    // light-giving tiles in view, nearest to Comic first (the shader-free grid takes at most MAX_LIGHTS)
    let (left, right) = (cam * 8.0 - 80.0, cam * 8.0 + 192.0 + ex + 80.0);
    let mut found: Vec<(f32, light::Light)> = vec![];
    for c in 0..map.w {
        let cx = c as f32 * 16.0 + 8.0;
        if cx < left || cx > right { continue; }
        for r in 0..map.h {
            let id = map.cells[r * map.w + c];
            if let Some(&(_, radius, strength)) = sources.iter().find(|(ids, _, _)| ids.contains(&id)) {
                let cy = r as f32 * 16.0 + 8.0;
                found.push((((cx - cx0).powi(2) + (cy - cy0).powi(2)), light::Light { x: cx, y: cy, radius, strength }));
            }
        }
    }
    found.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (_, l) in found { if lights.len() < light::MAX_LIGHTS { lights.push(l); } }
    Some((ambient, lights))
}

/// Draw a HUD sprite with whole screen pixels per source pixel (`p` across, `py` down) at a screen position.
fn blit_hud(t: &Texture2D, sx: f32, sy: f32, p: f32, py: f32) {
    draw_texture_ex(t, sx, sy, WHITE, DrawTextureParams { dest_size: Some(vec2(t.width() * p, t.height() * py)), ..Default::default() });
}

/// The immersive layout: score top right, shield bar top left, lives bottom left and collected items bottom right,
/// drawn straight over the world (no boxes). `size` is the HUD size relative to the window scale: every source pixel
/// becomes a whole number of screen pixels, so the sprites stay crisp at any window size.
fn draw_overlay_hud(g: &Game, sp: &Sprites, f: &Frame, fx: Option<&fx::Fx>, size: f32) {
    let p = (size * f.s).round().max(1.0);
    let py = (size * f.sy).round().max(1.0);
    let (l, t) = ((f.x + 8.0 * f.s).round(), (f.y + 8.0 * f.sy).round());
    let (r, b) = ((f.x + (8.0 + 192.0 + f.ex) * f.s).round(), (f.y + 168.0 * f.sy).round());
    let (mx, my) = ((3.0 * f.s).round(), (3.0 * f.sy).round());
    let score = fx.map_or(g.score, |e| e.shown_score.round() as u32) as u64 * 100;
    for i in 0..8u32 {
        let d = (score / 10u64.pow(7 - i)) % 10;
        blit_hud(&sp.ov_digits[d as usize], r - mx - (8 - i) as f32 * 8.0 * p, t + my, p, py);
    }
    for j in 1..=6 {
        blit_hud(&sp.ov_pips[if g.hp >= j { 2 } else { 0 }], l + mx + (j - 1) as f32 * 8.0 * p, t + my, p, py);
    }
    for i in 1..=5 {
        blit_hud(&sp.ov_life[if g.lives >= i { 0 } else { 1 }], l + mx + (i - 1) as f32 * 17.0 * p, b - my - 16.0 * py, p, py);
    }
    let fr = g.hud_icon_frame();
    let mut icons: Vec<usize> = vec![];
    if g.max_fireballs > 0 { icons.push(8 + g.max_fireballs.min(5)); }
    for (has, k) in [(g.items.corkscrew, 0), (g.items.key, 1), (g.items.boots, 2), (g.items.lantern, 3), (g.items.wand, 4)] { if has { icons.push(k); } }
    for (i, got) in g.treasure_flags.iter().enumerate() { if *got { icons.push(5 + i); } }
    if icons.is_empty() { return; }
    let w = icons.len() as f32 * 17.0 * p + p;
    let (x0, y0) = (r - mx - w, b - my - 16.0 * py);
    draw_rectangle(x0 - 2.0 * p, y0 - 2.0 * py, w + 4.0 * p, 16.0 * py + 4.0 * py, Color::new(0.5, 0.5, 0.5, 0.4));
    for (n, k) in icons.iter().enumerate() { blit_hud(&sp.items[fr][*k], x0 + p + n as f32 * 17.0 * p, y0, p, py); }
}

fn draw_play(g: &Game, f: &Frame, sp: &Sprites, gfx: &[LevelGfx], paused: bool, sm: Option<(&Snap, f32)>, hint: bool, fx: Option<&fx::Fx>, vis: Option<&[(f32, f32)]>, imm: bool, hud: f32, lighting: Option<&light::Lighting>) {
    // Full-screen story text during the ending, faded in like the title.
    if let Phase::Ending(Ending::Screen(t)) = g.phase {
        blit(sp.fades[2].get(t as usize).unwrap_or(&sp.screens[2]), f, 0.0, 0.0);
        return;
    }

    let fx_ref = fx;
    let (mut px, mut py, mut cam) = (g.player.x as f32, g.player.y as f32, g.player.cam as f32);
    // smooth motion: only within one stage of normal play
    let sm = sm.filter(|(sn, _)| g.phase == Phase::Playing && g.dying.is_none() && g.tele.is_none() && sn.resets == g.resets && sn.level == g.level_idx && sn.stage == g.stage_idx);
    if let Some((sn, a)) = sm {
        px = mix(sn.x, g.player.x, a); py = mix(sn.y, g.player.y, a); cam = mix(sn.cam, g.player.cam, a);
    }

    // window frame (score panel etc.) and a black playfield
    if !imm { blit(if f.ex > 0.0 { &sp.frame_wide } else { &sp.screens[3] }, f, 0.0, 0.0); }
    let fh = Frame { x: f.x + f.ex * f.s, ..*f }; // the score panel moves right in widescreen
    let fl = Frame { x: f.x + f.ex / 2.0 * f.s, ..*f }; // the lives box is centred under the wider playfield
    let wu = (f.ex / 16.0).round(); // extra units visible on each side
    let view = f.px(8.0, 8.0, VIEW_W + f.ex, VIEW_H);
    draw_rectangle(view.0, view.1, view.2, view.3, BLACK);
    // at fractional scales a stray pixel of the frame texture can show right beside the playfield; the border there is black
    if !imm { for (x, y, w, h) in [(view.0 + view.2, view.1, 1.0, view.3), (view.0 - 1.0, view.1, 1.0, view.3), (view.0, view.1 - 1.0, view.2, 1.0), (view.0, view.1 + view.3, view.2, 1.0)] {
        draw_rectangle(x, y, w, h, BLACK);
    } }

    // clip everything in the playfield to its 192x160 window
    unsafe {
        get_internal_gl().quad_gl.scissor(Some((
            view.0 as i32, view.1 as i32, view.2 as i32, view.3 as i32)));
    }
    // during a door animation the scene shown (and its camera) can differ from the game's current stage
    let door = if let Phase::Door(d) = g.phase { Some(d) } else { None };
    let (lvl, stg, cam) = match door { Some(d) => { let (l, s, c) = d.scene(); (l, s, c as f32) } None => (g.level_idx, g.stage_idx, cam) };
    let lg = &gfx[lvl];
    let map = &g.maps[lvl][stg];
    // widescreen: a 40-unit view that follows the game's camera but never shows past the ends of the map
    let wide = f.ex > 0.0;
    let cam = if wide { (cam - wu).clamp(0.0, ((map.w * 2) as f32 - (24.0 + f.ex / 8.0)).max(0.0)) } else { cam };
    let f = &Frame { wx: 0.0, ..*f };
    let dark = lvl == 6 && !g.items.lantern;
    let cam_px = cam * 8.0;
    if !dark {
        let first = ((cam_px - f.wx) / 16.0).floor().max(0.0) as usize;
        for c in first..(first + ((192.0 + f.ex) / 16.0).ceil() as usize + 2).min(map.w) {
            for r in 0..map.h {
                let tx = 8.0 + f.wx + c as f32 * 16.0 - cam_px;
                if tx > 8.0 + VIEW_W + f.ex - 1.0 { continue; } // a sliver of the next column must not show at the edge
                if let Some(tl) = lg.tiles.get(map.cells[r * map.w + c] as usize) {
                    blit(tl, f, tx, 8.0 + r as f32 * 16.0);
                }
            }
        }
    }

    // ambient effects (Effects): water glints on the lake, twinkling stars in space
    if let (Some(e), false) = (fx_ref, dark) {
        let vpx = (8.0, 8.0, 8.0 + 192.0 + f.ex, 8.0 + 160.0);
        // the lit castle is dim except around Comic (drawn first so the torch glows stay bright)
        if lvl == 6 && g.items.lantern && lighting.is_none() { e.darkness(f, 8.0 + (px - cam) * 8.0 + 8.0, 8.0 + py * 8.0 + 16.0, vpx); }
        e.draw_ambient(f, lvl, map, cam, lg.spots.as_deref());
        e.draw_mist(f, lvl, map, cam);
        e.draw_weather(f, lvl);
    }

    // the door animation: black doorway with the two door-frame posts, Comic walking through (original 0x0d25 / 0x0ca8)
    if let Some(d) = door {
        let (dy, dx) = d.door;
        let (x, y) = (8.0 + f.wx + (dx as f32 - cam) * 8.0, 8.0 + dy as f32 * 8.0);
        let tiles = g.levels[lvl].door_tiles;
        for op in d.ops().iter().skip(1) {
            match op {
                DoorOp::Clear => { let r = f.px(x, y, 32.0, 32.0); draw_rectangle(r.0, r.1, r.2, r.3, BLACK) }
                DoorOp::Edges => {
                    // right half of tiles 0/2 on the left, left half of tiles 1/3 on the right
                    for (i, (src_x, ox, oy)) in [(8.0, 0.0, 0.0), (0.0, 24.0, 0.0), (8.0, 0.0, 16.0), (0.0, 24.0, 16.0)].iter().enumerate() {
                        if let Some(tl) = lg.tiles.get(tiles[i] as usize) {
                            let r = f.px(x + ox, y + oy, 8.0, 16.0);
                            draw_texture_ex(tl, r.0, r.1, WHITE, DrawTextureParams {
                                source: Some(Rect::new(*src_x, 0.0, 8.0, 16.0)), dest_size: Some(vec2(r.2, r.3)), ..Default::default() });
                        }
                    }
                }
                DoorOp::Player => blit_unit(&sp.player[g.player.frame()], f, g.player.x as f32, g.player.y as f32, cam),
                DoorOp::Playfield => {}
            }
        }
        // light spilling out of the doorway while the door is open (Effects)
        if let Some(e) = fx_ref {
            let k = if d.arriving { (1.0 - d.frame as f32 / 8.0).max(0.0) } else { (0.35 + 0.15 * d.frame as f32).min(1.0) };
            e.glow(f, x + 16.0, y + 16.0, 40.0, Color::new(1.0, 0.92, 0.55, 0.45 * k));
            e.glow(f, x + 16.0, y + 30.0, 26.0, Color::new(1.0, 0.95, 0.7, 0.35 * k));
        }
    }

    // item (drawn while the world is live and during the opening, not during door/ending/game-over screens)
    if let (true, Some((kind, iy, ix))) = (matches!(g.phase, Phase::Opening(_) | Phase::Playing) && !paused, g.stage().item) {
        if !g.collected.contains(&(g.level_idx, g.stage_idx)) {
            if let Some(fx) = fx {
                let pulse = 0.55 + 0.25 * (fx.time * 4.0).sin();
                fx.glow(f, 8.0 + (ix as f32 - cam) * 8.0 + 8.0, 8.0 + iy as f32 * 8.0 + 8.0, 16.0, Color::new(1.0, 0.95, 0.5, pulse * 0.6));
            }
            blit_unit(&sp.items[g.item_frame()][kind as usize & 15], f, ix as f32, iy as f32, cam);
        }
    }

    let (opening, ending_beam) = match g.phase {
        Phase::Opening(t) => (Some(t), None),
        Phase::Ending(Ending::BeamOut(t)) => (None, Some(t)),
        _ => (None, None),
    };
    let world_live = matches!(g.phase, Phase::Playing) && !paused;

    // enemies and fireballs only exist during play
    if world_live {
        for (i, e) in g.enemies.iter().enumerate() {
            if e.state == 0 { continue; }
            let (mut ex, mut ey) = (e.x as f32, e.y as f32);
            if let Some(&(vx, vy)) = vis.and_then(|v| v.get(i)) { ex = vx; ey = vy; }
            else if let Some((sn, a)) = sm { if let Some(&(py0, px0)) = sn.enemies.get(i) { ex = mix(px0, e.x, a); ey = mix(py0, e.y, a); } }
            // the original draws an enemy only when its column is 0..=22 units from the camera's left edge
            if wide { if !(-2.0..=24.0 + f.ex / 8.0 + 2.0).contains(&(e.x as f32 - cam)) { continue; } } else if !(0..=0x16).contains(&(e.x - g.player.cam)) { continue; }
            let slot = e.frame + e.face;
            // State 7 lasts one tick between the end of the explosion and the enemy being removed. The original draws the
            // live sprite then, so a shot enemy flashes back for a moment; with any enhancement on it stays hidden (an
            // enemy that fell out of the bottom of the stage, at y 18, still shows).
            let enhanced = fx_ref.is_some() || sm.is_some() || vis.is_some();
            let (show_enemy, boom) = match e.state {
                1 => (true, None),
                7 => (!enhanced || e.y >= 0x12, None),
                s => { let bl = if s > 7 { s - 6 } else { s }; (bl <= 4, Some((bl - 2) as usize)) }
            };
            if show_enemy {
                if let Some(Some(tx)) = lg.enemies.get(e.sprite).and_then(|s| s.get(slot)) {
                    match fx_ref.map(|x| x.spawn_alpha(i)).filter(|a| *a < 1.0) {
                        Some(a) => {
                            // appearing: a soft light grows while the enemy fades in
                            if let Some(x) = fx_ref { x.glow(f, 8.0 + (ex - cam) * 8.0 + 8.0, 8.0 + ey * 8.0 + 8.0, 10.0 + 14.0 * (1.0 - a), Color::new(0.75, 0.8, 1.0, 0.7 * (1.0 - a))); }
                            blit_unit_a(tx, f, ex, ey, cam, a);
                        }
                        None => blit_unit(tx, f, ex, ey, cam),
                    }
                }
            }
            if let Some(b) = boom { if let Some(tx) = sp.explosion.get(b) { blit_unit(tx, f, ex, ey, cam); } }
        }
        for (i, fb) in g.fireballs.iter().enumerate() {
            let Some(fb) = fb else { continue };
            let (mut fx, mut fy) = (fb.x as f32, fb.y as f32);
            if let Some((sn, a)) = sm {
                // a fireball that appeared this tick starts one step behind, so it flies from the first frame
                let (y0, x0) = sn.fbs[i].unwrap_or((fb.y, fb.x - fb.dx));
                fx = mix(x0, fb.x, a); fy = mix(y0, fb.y, a);
            }
            if let Some(e) = fx_ref {
                e.glow(f, 8.0 + (fx - cam) * 8.0 + 8.0, 8.0 + fy * 8.0 + 4.0, 10.0, Color::new(1.0, 0.6, 0.2, 0.35));
            }
            blit_unit(&sp.fireball[fb.frame % 2], f, fx, fy, cam);
        }
    }

    // player (with the opening / ending / death effects)
    let mut show_player = world_live && g.phase != Phase::GameOver;
    let mut overlay: Option<&Texture2D> = None;
    if let Some(o) = opening {
        // 15 empty ticks, a wait for the start jingle, then 12 beam frames (Comic appears halfway), then 6 ticks standing
        show_player = o >= 16 + 6;
        if (16..28).contains(&o) { overlay = Some(&sp.beam[(o - 16) as usize]); }
    }
    if let Some(b) = ending_beam {
        show_player = b < 6;
        if b < 12 { overlay = Some(&sp.beam[b as usize]); }
    }
    if let Some(n) = g.dying {
        let k = (9 - n.min(9)) as usize;
        show_player = k < 4;
        overlay = sp.death.get(k);
    }
    if let (Some(e), true, false) = (fx_ref, show_player, g.player.air) {
        e.shadow(f, 8.0 + (px - cam) * 8.0 + 8.0, 8.0 + (py + 4.0) * 8.0 + 1.0);
    }
    if show_player { blit_unit(&sp.player[g.player.frame()], f, px, py, cam); }
    if let Some(ov) = overlay { blit_unit(ov, f, px, py, cam); }
    // falling off the bottom (original 0x142f): for one tick Comic sinks out of the screen in the jump pose, then the
    // playfield is empty (the playfield clip does what the original's row-limited draw does)
    if let Phase::Fall(0) = g.phase { blit_unit(&sp.player[(g.player.facing + 4) as usize], f, px, py, cam); }
    if let Some(tp) = &g.tele {
        let s = tp.step as usize;
        if s != 5 { blit_unit(&sp.puff[s.min(4)], f, tp.orig.1 as f32, tp.orig.0 as f32, cam); }
        if s >= 1 { blit_unit(&sp.puff[(s - 1).min(4)], f, tp.dest.1 as f32, tp.dest.0 as f32, cam); }
    }
    // ray-traced lighting (prototype): the lit castle and the cave
    if let Some(lt) = lighting {
        if let Some((ambient, lights)) = scene_lights(g, lvl, map, px, py, cam, f.ex) {
            lt.draw(f, (lvl, stg), map, (8.0, 8.0, 192.0 + f.ex, 160.0), cam * 8.0, ambient, &lights);
        }
    }
    if let Some(e) = fx_ref {
        e.draw_particles(f, cam);
        if e.flash > 0.0 { draw_rectangle(view.0, view.1, view.2, view.3, Color::new(1.0, 0.1, 0.1, 0.30 * e.flash)); }
        e.iris(f, 8.0 + (px - cam) * 8.0 + 8.0, 8.0 + py * 8.0 + 16.0, (8.0, 8.0, 8.0 + 192.0 + f.ex, 8.0 + 160.0));
    }
    if matches!(g.phase, Phase::GameOver | Phase::Ending(Ending::Banner)) { blit(&sp.banner, f, 8.0 + f.ex / 2.0 + 32.0, 8.0 + 56.0); }
    if paused { blit(&sp.pause, f, 8.0 + f.ex / 2.0 + 32.0, 8.0 + 56.0); }
    if hint {
        let r = f.px(8.0 + f.ex / 2.0 + 8.0, 8.0 + 8.0, 176.0, 16.0);
        draw_rectangle(r.0, r.1, r.2, r.3, BLACK);
        blit(&sp.hint, f, 8.0 + f.ex / 2.0 + 12.0, 8.0 + 12.0);
    }
    unsafe { get_internal_gl().quad_gl.scissor(None); }

    if imm { draw_overlay_hud(g, sp, f, fx_ref, hud); return; }
    // ---------------------------------------------------------------- HUD (positions from the original)
    let score = fx_ref.map_or(g.score, |e| e.shown_score.round() as u32) as u64 * 100;
    for i in 0..8u32 {
        let d = (score / 10u64.pow(7 - i)) % 10;
        blit(&sp.digits[d as usize], &fh, (29 + i) as f32 * 8.0, 24.0);
    }
    for j in 1..=6 {
        blit(&sp.pips[if g.hp >= j { 2 } else { 0 }], &fh, (30 + j) as f32 * 8.0, 82.0);
        let amt = (g.meter - 2 * (j - 1)).clamp(0, 2) as usize;
        blit(&sp.pips[amt], &fh, (30 + j) as f32 * 8.0, 54.0);
    }
    for i in 1..=5 {
        blit(&sp.life[if g.lives >= i { 0 } else { 1 }], &fl, (3 + 3 * i) as f32 * 8.0, 180.0);
    }
    let slots: [(bool, usize, f32, f32); 5] = [
        (g.items.corkscrew, 0, 256.0, 112.0), (g.items.key, 1, 280.0, 112.0), (g.items.boots, 2, 232.0, 136.0),
        (g.items.lantern, 3, 256.0, 136.0), (g.items.wand, 4, 280.0, 136.0),
    ];
    for (has, k, x, y) in slots { if has { blit(&sp.items[g.hud_icon_frame()][k], &fh, x, y); } }
    for (i, got) in g.treasure_flags.iter().enumerate() { if *got { blit(&sp.items[g.hud_icon_frame()][5 + i], &fh, 232.0 + 24.0 * i as f32, 160.0); } }
    if g.max_fireballs > 0 { blit(&sp.items[g.hud_icon_frame()][8 + g.max_fireballs.min(5)], &fh, 232.0, 112.0); }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> Option<exe::Exe> {
        let p = std::path::Path::new("../AdvCapCo/comic.exe");
        p.exists().then(|| exe::Exe::load(p).unwrap())
    }

    fn entry_screen() -> (HighScore, std::path::PathBuf) {
        let bg = assets::Image { w: 320, h: 200, rgba: [0, 0, 0, 255].repeat(320 * 200) };
        let path = std::env::temp_dir().join(format!("comic_hgh_test_{}_{:?}.hgh", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_file(&path);
        (HighScore::new(&bg, None, &path, 5, true), path)
    }

    fn typed(s: &str) -> NameInput { NameInput { chars: s.chars().collect(), ..Default::default() } }

    #[test]
    fn name_entry_takes_the_keyboard_and_the_controller_at_the_same_time() {
        let (mut h, path) = entry_screen();
        assert!(h.entry.is_some(), "a score of 5 gets a place in an empty table");
        h.input(None, &path, &typed("Ab"));
        h.input(None, &path, &NameInput { up: true, ..Default::default() });       // controller: picks A
        h.input(None, &path, &NameInput { up: true, ..Default::default() });       // B
        h.input(None, &path, &NameInput { add: true, ..Default::default() });      // adds B
        assert_eq!(h.name, b"AbB");
        h.input(None, &path, &NameInput { backspace: true, ..Default::default() }); // keyboard deletes the B
        h.input(None, &path, &typed("c"));                                          // keyboard again
        h.input(None, &path, &NameInput { del: true, ..Default::default() });       // controller deletes the c
        assert_eq!(h.name, b"Ab");
        h.input(None, &path, &NameInput { down: true, ..Default::default() });      // pick (wraps to the last letter, '-')
        h.input(None, &path, &NameInput { start: true, ..Default::default() });     // Start finishes and counts the pick
        assert_eq!(h.name, b"Ab-");
        assert!(h.entry.is_none(), "the table is shown");
        assert!(h.input(None, &path, &NameInput::default()), "and the screen reports it");
        let saved = std::fs::read(&path).unwrap();
        assert_eq!(&saved[..3], b"Ab-");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_letter_that_was_added_is_not_added_again_by_finishing() {
        let (mut h, path) = entry_screen();
        h.input(None, &path, &NameInput { up: true, ..Default::default() });
        h.input(None, &path, &NameInput { add: true, ..Default::default() });
        h.input(None, &path, &NameInput { enter: true, ..Default::default() });
        assert_eq!(h.name, b"A");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_name_stops_at_twelve_characters_from_either_device() {
        let (mut h, path) = entry_screen();
        h.input(None, &path, &typed("abcdefghij"));
        for _ in 0..4 {
            h.input(None, &path, &NameInput { up: true, ..Default::default() });
            h.input(None, &path, &NameInput { add: true, ..Default::default() });
        }
        h.input(None, &path, &typed("zzz"));
        assert_eq!(h.name.len(), hiscore::NAME_MAX);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn no_stray_letter_is_left_on_the_screen_when_the_two_devices_mix() {
        let Some(font) = hiscore::Font::load() else { return };
        let (mut h, path) = entry_screen();
        // the name starts at column 22 of row 11 (after "Enter Your Name: ")
        let ink = |h: &HighScore, col: usize| -> bool {
            (0..8).any(|y| (0..8).any(|x| { let o = ((11 * 8 + y) * h.ts.img.w + col * 8 + x) * 4; h.ts.img.rgba[o..o + 3] != [0, 0, 0] }))
        };
        h.input(Some(&font), &path, &NameInput { up: true, ..Default::default() });
        assert!(ink(&h, 22), "the picked letter shows at the cursor");
        h.input(Some(&font), &path, &typed("X"));
        assert!(ink(&h, 22) && !ink(&h, 23), "a typed letter replaces it and nothing is left behind it");
        h.input(Some(&font), &path, &NameInput { up: true, ..Default::default() });
        assert!(ink(&h, 23), "the picked letter moves to the next cell");
        h.input(Some(&font), &path, &NameInput { backspace: true, ..Default::default() });
        assert!(!ink(&h, 22) || h.cand.is_some(), "after deleting, only the picked letter is shown");
        assert!(!ink(&h, 23), "and none is left in the cell after it");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn notice_and_key_names_come_from_the_exe() {
        let Some(exe) = exe() else { return };
        let n = notice_text(&exe);
        assert!(n.contains("The Adventures of Captain Comic  --  Revision 3"));
        assert!(n.contains("Press 'K' to define the keyboard"));
        assert_eq!(key_name(&exe, 0x39).trim(), "Space");
        assert_eq!(key_name(&exe, 0x52).trim(), "Ins");
        assert_eq!(key_name(&exe, 0x38).trim(), "Alt");
    }

    #[test]
    fn key_dialog_collects_six_distinct_keys_then_asks_to_confirm_and_save() {
        let mut kc = KeyConfig::default();
        assert_eq!(kc.key(KeyCode::Escape), None); // Esc is rejected
        assert!(kc.got.is_empty());
        for k in [KeyCode::A, KeyCode::A, KeyCode::D, KeyCode::W, KeyCode::F, KeyCode::S, KeyCode::T] {
            assert_eq!(kc.key(k), None); // the repeated A is ignored
        }
        assert_eq!(kc.stage, 1);
        let b = kc.bindings();
        assert_eq!((b.left, b.right, b.jump, b.fire, b.open, b.teleport), (0x1e, 0x20, 0x11, 0x21, 0x1f, 0x14));
        kc.key(KeyCode::N); // "setup OK? n" starts over
        assert_eq!((kc.stage, kc.got.len()), (0, 0));
        for k in [KeyCode::Left, KeyCode::Right, KeyCode::Space, KeyCode::Insert, KeyCode::LeftAlt, KeyCode::CapsLock] { kc.key(k); }
        kc.key(KeyCode::Y);
        assert_eq!(kc.stage, 2);
        assert_eq!(kc.key(KeyCode::Y), Some(true)); // save
        assert_eq!(kc.bindings(), input::Bindings::DEFAULT);
    }

    #[test]
    fn terminal_handles_cr_lf_like_dos() {
        let mut t = Term::new();
        t.put("ab\r\ncd\n  e");
        assert_eq!(t.cells[0][..2], ['a', 'b']);
        assert_eq!(t.cells[1][..2], ['c', 'd']);
        assert_eq!(t.cells[2][4], 'e', "a lone LF keeps the column (2 after 'cd', then 2 spaces)");
    }
}
