//! Headless trace of the Rust port, for differential testing against the original (work/emu).
//! Usage: trace <asset dir> <script> [level] [cheat]
//! Script lines: "<ticks> <key,key,...>" with keys left,right,jump,fire,open,tele (or "-" for none).
#[path = "assets.rs"] #[allow(dead_code)] mod assets;
#[path = "data.rs"] #[allow(dead_code)] mod data;
#[path = "exe.rs"] mod exe;
#[path = "game.rs"] #[allow(dead_code)] mod game;
#[path = "sound.rs"] #[allow(dead_code)] mod sound;

use game::{Game, Input, Map, Phase};
use std::path::PathBuf;

fn line(g: &Game) -> String {
    let p = &g.player;
    let mut s = format!("y={} x={} cam={} vx={} vy={} air={} face={} anim={} hp={} lives={} meter={} score={} level={} stage={} walk={} sp={},{} jc={}",
        p.y, p.x, p.cam, p.vx, p.vy, p.air as i32, p.facing, p.anim, g.hp, g.lives, g.meter, g.score, g.level_idx, g.stage_idx, p.walk_phase(), g.spawn_counters().0, g.spawn_counters().1, p.jump_counter());
    s.push_str(" E:");
    for e in &g.enemies { s.push_str(&format!("{},{},{};", e.state, e.y, e.x)); }
    s.push_str(" D:");
    for e in &g.enemies {
        let plus4 = if e.state == 0 { e.timer } else { e.frame as i32 };
        s.push_str(&format!("{},{},{},{},{};", (e.dx as i8) as u8, (e.vy as i8) as u8, e.face, e.toggle, plus4));
    }
    s.push_str(" F:");
    for f in &g.fireballs { match f { Some(f) => s.push_str(&format!("{},{};", f.y, f.x)), None => s.push_str("-;") } }
    s
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(&a[1]);
    let script = std::fs::read_to_string(&a[2]).expect("script");
    let level: usize = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(1);
    let cheat = a.get(4).map_or(false, |v| v == "cheat");

    let exe = exe::Exe::load(&dir.join("comic.exe")).expect("comic.exe");
    let levels: Vec<data::Level> = (0..data::LEVEL_COUNT).map(|i| data::level(&exe, i)).collect();
    let maps: Vec<Vec<Map>> = levels.iter().map(|l| {
        let ts = assets::load_tileset(&dir.join(&l.tileset)).unwrap();
        l.maps.iter().map(|m| {
            let m = assets::load_map(&dir.join(m)).unwrap();
            Map { cells: m.cells, w: m.w, h: m.h, solid_above: ts.header[0] }
        }).collect()
    }).collect();
    let lens = sound::ALL.iter().map(|&s| (s, sound::total_ticks(&exe, s))).collect();
    let mut g = Game::new(levels, maps, lens);
    g.phase = Phase::Playing;
    if level != 1 { g.debug_goto(level); }
    if a.get(5).map_or(false, |v| v == "lives5") { g.lives = 5; }
    if cheat { g.max_fireballs = 5; g.items.key = true; g.items.corkscrew = true; g.items.boots = true; g.items.wand = true; g.items.lantern = true; g.player.reload_jump(5); }
    println!("{}", line(&g));
    for l in script.lines() {
        let mut it = l.split_whitespace();
        let (Some(n), Some(keys)) = (it.next(), it.next()) else { continue };
        let n: u32 = n.parse().unwrap();
        let has = |k: &str| keys.split(',').any(|x| x == k);
        let inp = Input { left: has("left"), right: has("right"), jump: has("jump"), open: has("open"), fire: has("fire"), teleport: has("tele") };
        for _ in 0..n {
            g.tick(inp);
            // the original's death and door animations block inside one logic tick; run them to the end like that
            while g.dying.is_some() || matches!(g.phase, Phase::Door(_) | Phase::Fall(_)) { g.tick(Input::default()); }
            if g.game_over { println!("GAMEOVER"); return; }
            println!("{}", line(&g));
        }
    }
}
