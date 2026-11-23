//! Loaders for the original Captain Comic (1988) data files.
use std::{fs, io, path::Path};

/// The game's palette. The original sets palette registers 10 and 12 to 0x1a and 0x1c, but it runs in a 200-line
/// mode (0Dh), where the monitor reads palette values as CGA RGBI (bit 4 = intensity, bits 3/5 ignored): those are
/// the standard light green and light red, so the palette is the plain 16-colour one.
pub const EGA: [[u8; 3]; 16] = [
    [0, 0, 0], [0, 0, 170], [0, 170, 0], [0, 170, 170],
    [170, 0, 0], [170, 0, 170], [170, 85, 0], [170, 170, 170],
    [85, 85, 85], [85, 85, 255], [85, 255, 85], [85, 255, 255],
    [255, 85, 85], [255, 85, 255], [255, 255, 85], [255, 255, 255],
];

/// RGBA pixels, row-major.
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

/// Four consecutive bit planes (each `w/8 * h` bytes) -> RGBA. `mask` set bits become transparent.
pub fn planes_to_rgba(planes: &[u8], mask: Option<&[u8]>, w: usize, h: usize) -> Image {
    let bpr = w / 8;
    let psz = bpr * h;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let bit = 7 - (x % 8);
            let o = y * bpr + x / 8;
            let mut c = 0usize;
            for p in 0..4 {
                c |= (((planes[p * psz + o] >> bit) & 1) as usize) << p;
            }
            let transparent = mask.map_or(false, |m| (m[o] >> bit) & 1 == 1);
            let [r, g, b] = EGA[c];
            rgba.extend_from_slice(&[r, g, b, if transparent { 0 } else { 255 }]);
        }
    }
    Image { w, h, rgba }
}

/// `.ega`: 2-byte header, then RLE (high bit = run of next byte, else literal run) -> 4 planes of a 320x200 screen.
pub fn load_ega(path: &Path) -> io::Result<Image> {
    let d = fs::read(path)?;
    let mut out = Vec::with_capacity(32000);
    let mut i = 2;
    while i < d.len() {
        let b = d[i];
        i += 1;
        let n = (b & 0x7f) as usize;
        if b & 0x80 != 0 {
            out.extend(std::iter::repeat(d[i]).take(n));
            i += 1;
        } else {
            out.extend_from_slice(&d[i..i + n]);
            i += n;
        }
    }
    out.resize(32000, 0);
    Ok(planes_to_rgba(&out, None, 320, 200))
}

/// `.tt2`: 4-byte header followed by 16x16 tiles (128 bytes each, 4 sequential planes).
pub struct Tileset {
    pub header: [u8; 4],
    pub tiles: Vec<Image>,
}

pub fn load_tileset(path: &Path) -> io::Result<Tileset> {
    let d = fs::read(path)?;
    let header = [d[0], d[1], d[2], d[3]];
    let tiles = d[4..].chunks_exact(128).map(|t| planes_to_rgba(t, None, 16, 16)).collect();
    Ok(Tileset { header, tiles })
}

/// `.pt`: u16 width, u16 height, then one tile id per cell (row-major).
pub struct TileMap {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<u8>,
}

pub fn load_map(path: &Path) -> io::Result<TileMap> {
    let d = fs::read(path)?;
    let w = u16::from_le_bytes([d[0], d[1]]) as usize;
    let h = u16::from_le_bytes([d[2], d[3]]) as usize;
    Ok(TileMap { w, h, cells: d[4..4 + w * h].to_vec() })
}

/// `.shp`: frames of 160 bytes = 4 planes of 32 bytes, then a 32-byte 16x16 mask.
pub fn load_sprite(path: &Path) -> io::Result<Vec<Image>> {
    let d = fs::read(path)?;
    Ok(d.chunks_exact(160).map(|f| planes_to_rgba(&f[..128], Some(&f[128..]), 16, 16)).collect())
}

/// Four sequential planes plus a separate mask (set bit = transparent), `w`x`h` pixels.
pub fn planes_to_rgba_masked(planes: &[u8], mask: &[u8], w: usize, h: usize) -> Image {
    planes_to_rgba(planes, Some(mask), w, h)
}
