//! Optional ray-traced lighting (prototype). The darkness of the playfield is computed on the CPU on a coarse grid
//! (4 px cells, anchored to the world so it does not shimmer when the camera moves): from each cell, rays are walked
//! toward every nearby light through the stage's solid tiles, so walls block light and cast soft shadows. The grid is
//! uploaded as a small black texture whose alpha is the darkness and drawn over the finished playfield with linear
//! filtering, which blends smoothly between the grid points. Cells inside a solid tile all use the tile's centre as
//! their starting point, so the surface of a wall or floor is lit evenly instead of fanning out inside each tile.
use crate::game::Map;
use macroquad::prelude::*;
use std::cell::RefCell;

pub const MAX_LIGHTS: usize = 12;
const CELL: f32 = 4.0;

/// A light in world pixels (x across the stage, y down the playfield) with its reach and strength.
#[derive(Clone, Copy)]
pub struct Light { pub x: f32, pub y: f32, pub radius: f32, pub strength: f32 }

pub struct Lighting {
    tex: RefCell<Option<((usize, usize), Texture2D)>>,
}

fn solid(map: &Map, tx: i32, ty: i32) -> bool {
    tx >= 0 && ty >= 0 && (tx as usize) < map.w && (ty as usize) < map.h && map.cells[ty as usize * map.w + tx as usize] > map.solid_above
}

/// 1 when no solid tile lies between `p` and `l` (the tiles holding the two end points do not block).
fn visible(map: &Map, p: (f32, f32), l: (f32, f32)) -> f32 {
    let (dx, dy) = (l.0 - p.0, l.1 - p.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 { return 1.0; }
    let tile = |x: f32, y: f32| ((x / 16.0).floor() as i32, (y / 16.0).floor() as i32);
    let (tp, tl) = (tile(p.0, p.1), tile(l.0, l.1));
    let (sx, sy) = (dx / len * 2.0, dy / len * 2.0);
    let steps = (len / 2.0) as i32;
    for i in 1..steps {
        let q = (p.0 + sx * i as f32, p.1 + sy * i as f32);
        let t = tile(q.0, q.1);
        if t == tp || t == tl { continue; }
        if solid(map, t.0, t.1) { return 0.0; }
    }
    1.0
}

/// Separable box blur of radius `r` cells (edges are clamped).
fn blur(src: &[f32], w: usize, h: usize, r: i32) -> Vec<f32> {
    let n = (2 * r + 1) as f32;
    let mut tmp = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for k in -r..=r { sum += src[y * w + (x as i32 + k).clamp(0, w as i32 - 1) as usize]; }
            tmp[y * w + x] = sum / n;
        }
    }
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for k in -r..=r { sum += tmp[(y as i32 + k).clamp(0, h as i32 - 1) as usize * w + x]; }
            out[y * w + x] = sum / n;
        }
    }
    out
}

impl Lighting {
    pub fn new() -> Lighting { Lighting { tex: RefCell::new(None) } }

    /// Darken the view `(x, y, w, h)` (virtual pixels, in frame `f`) except around the given lights.
    pub fn draw(&self, f: &crate::Frame, key: (usize, usize), map: &Map, view: (f32, f32, f32, f32), cam_px: f32, ambient: f32, lights: &[Light]) {
        let start = (cam_px / CELL).floor() * CELL; // world x of the first cell's left edge
        let cols = ((view.2 + CELL) / CELL).ceil() as usize + 1;
        let rows = (view.3 / CELL).ceil() as usize;
        let mut grid = vec![0.0f32; cols * rows];
        let lights = &lights[..lights.len().min(MAX_LIGHTS)];
        for ry in 0..rows {
            for cx in 0..cols {
                let mut p = (start + (cx as f32 + 0.5) * CELL, (ry as f32 + 0.5) * CELL);
                let (tx, ty) = ((p.0 / 16.0).floor() as i32, (p.1 / 16.0).floor() as i32);
                if solid(map, tx, ty) { p = (tx as f32 * 16.0 + 8.0, ty as f32 * 16.0 + 8.0); } // one value for a whole solid tile
                let mut light = 0.0f32;
                for l in lights {
                    let (dx, dy) = (l.x - p.0, l.y - p.1);
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist > l.radius { continue; }
                    let a = 1.0 - dist / l.radius;
                    let att = a * a * (3.0 - 2.0 * a);
                    // five samples across the light's width give soft shadow edges
                    let (px, py) = (-dy / dist.max(1.0) * 9.0, dx / dist.max(1.0) * 9.0);
                    let mut v = visible(map, p, (l.x, l.y));
                    for k in [-1.0f32, -0.5, 0.5, 1.0] { v += visible(map, p, (l.x + px * k, l.y + py * k)); }
                    light += att * l.strength * v / 5.0;
                }
                grid[ry * cols + cx] = light;
            }
        }
        // two box blurs (about 28 px across) round off the steps where light meets a tile edge
        for _ in 0..2 { grid = blur(&grid, cols, rows, 3); }
        let mut bytes = vec![0u8; cols * rows * 4];
        for (i, light) in grid.iter().enumerate() {
            // a soft roll-off instead of a hard cap keeps the edge of a bright pool gentle
            let dark = (1.0 - ambient) * (-light * 1.8).exp();
            bytes[i * 4 + 3] = (dark * 255.0) as u8;
        }
        let mut cache = self.tex.borrow_mut();
        let size = (cols, rows);
        if cache.as_ref().map_or(true, |(s, _)| *s != size) {
            let t = Texture2D::from_rgba8(cols as u16, rows as u16, &bytes);
            t.set_filter(FilterMode::Linear);
            *cache = Some((size, t));
        } else {
            let t = &cache.as_ref().unwrap().1;
            t.update(&Image { bytes, width: cols as u16, height: rows as u16 });
        }
        let _ = key;
        let t = &cache.as_ref().unwrap().1;
        let r = f.px(view.0 + start - cam_px, view.1, cols as f32 * CELL, rows as f32 * CELL);
        draw_texture_ex(t, r.0, r.1, WHITE, DrawTextureParams { dest_size: Some(vec2(r.2, r.3)), ..Default::default() });
    }
}
