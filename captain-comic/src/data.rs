//! Level records and stage descriptors stored inside comic.exe.
use crate::exe::Exe;

pub const LEVEL_COUNT: usize = 8;
/// Level index -> name, in the original order.
#[allow(dead_code)] // kept for tools and tests
pub const LEVEL_NAMES: [&str; LEVEL_COUNT] = ["lake", "forest", "space", "base", "cave", "shed", "castle", "comp"];

#[derive(Clone, Copy, Debug)]
pub struct Door { pub y: u8, pub x: u8, pub level: u8, pub stage: u8 }

#[derive(Clone, Debug)]
pub struct Stage {
    pub item: Option<(u8, u8, u8)>, // (type, y, x)
    pub left: Option<u8>,
    pub right: Option<u8>,
    pub doors: Vec<Door>,
    pub enemies: Vec<(u8, u8)>, // (sprite index, behaviour)
}

#[derive(Clone, Debug)]
pub struct SpriteDef { pub frames: usize, pub mode: u8, pub extra: usize, pub name: String }

#[derive(Clone, Debug)]
pub struct Level {
    pub door_tiles: [u8; 4], // tile ids of the door-frame pieces (record +0x38)
    pub sprites: Vec<SpriteDef>,
    pub tileset: String,
    pub maps: [String; 3],
    pub stages: [Stage; 3],
}

fn name(b: &[u8]) -> String {
    String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).trim().to_string()
}

pub fn level(exe: &Exe, index: usize) -> Level {
    let r = exe.seg(0x1f52, index * 0xd0, 0xd0);
    let opt = |v: u8| if v == 0xff { None } else { Some(v) };
    let stage = |s: usize| {
        let d = &r[0x80 + s * 0x19..0x80 + (s + 1) * 0x19];
        Stage {
            item: if d[0] == 0xff { None } else { Some((d[0], d[1], d[2])) },
            left: opt(d[3]),
            right: opt(d[4]),
            doors: (0..3).map(|i| &d[5 + i * 4..9 + i * 4]).filter(|b| b[0] != 0xff)
                .map(|b| Door { y: b[0], x: b[1], level: b[2], stage: b[3] }).collect(),
            enemies: (0..4).map(|i| (d[0x11 + i * 2], d[0x12 + i * 2])).collect(),
        }
    };
    let mut sprites = vec![];
    for k in 0..4 {
        let e = &r[0x3c + k * 0x11..0x3c + (k + 1) * 0x11];
        if e[0] == 0 { break; }
        sprites.push(SpriteDef { frames: e[0] as usize, mode: e[1], extra: e[2] as usize, name: name(&e[3..]) });
    }
    Level {
        door_tiles: [r[0x38], r[0x39], r[0x3a], r[0x3b]],
        sprites,
        tileset: name(&r[0..14]),
        maps: [name(&r[14..28]), name(&r[28..42]), name(&r[42..56])],
        stages: [stage(0), stage(1), stage(2)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn exe() -> Option<Exe> {
        let p = Path::new("../AdvCapCo/comic.exe");
        p.exists().then(|| Exe::load(p).expect("unpack"))
    }

    #[test]
    fn reads_level_records_from_the_original_exe() {
        let Some(exe) = exe() else { return };
        let lake = level(&exe, 0);
        assert_eq!(lake.tileset, "lake.tt2");
        assert_eq!(lake.maps[2], "lake2.pt");
        assert_eq!(lake.stages[0].item, Some((8, 0x0c, 0x70)));
        assert_eq!(lake.stages[0].left, Some(1));
        assert_eq!(lake.stages[0].right, None);
        assert_eq!(lake.stages[0].doors.len(), 2);
        let base = level(&exe, 3);
        assert_eq!(base.tileset, "base.tt2");
        assert_eq!(base.stages[0].doors.len(), 3);
    }
}
