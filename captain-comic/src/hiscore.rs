//! The "Famous Comics" high-score table (original 0x2190), shown after the game ends.
//!
//! COMIC.HGH: 10 entries of 18 bytes: name (up to 12 chars, '$'-terminated), byte 13 unused, score as three
//! base-100 digits at +14..+16 (least significant first, the displayed score has "00" appended), byte 17 unused.
//! Missing file: every entry is 13 spaces + '$' with a zero score.
//!
//! The screen is mode 0Dh (40x25 text cells of 8x8) with sys005.ega loaded; all text goes through BIOS teletype,
//! which in planar modes paints the whole 8x8 cell (glyph in the colour, background black).
use crate::assets::{Image, EGA};
use std::path::{Path, PathBuf};

pub const ENTRY: usize = 18;
pub const ENTRIES: usize = 10;
pub const NAME_MAX: usize = 12; // DOS buffered input with a 13-byte buffer (incl. CR)

pub fn default_table() -> Vec<u8> {
    let mut t = vec![0u8; ENTRY * ENTRIES];
    for e in t.chunks_mut(ENTRY) { e[..13].fill(b' '); e[13] = b'$'; }
    t
}

pub fn load(path: &Path) -> Vec<u8> {
    match std::fs::read(path) {
        Ok(mut d) if !d.is_empty() => { d.resize(ENTRY * ENTRIES, 0); d }
        _ => default_table(),
    }
}

/// The score as the original stores it ([0x4e..0x50]): three base-100 digits, least significant first.
pub fn score_bytes(score: u32) -> [u8; 3] {
    [(score % 100) as u8, (score / 100 % 100) as u8, (score / 10000 % 100) as u8]
}

/// Where a score goes in the table (0x21e3): the first entry whose score is lower, comparing the most significant
/// digit first (signed byte compares, equal scores do not displace an entry). None: not a high score.
pub fn rank(table: &[u8], score: [u8; 3]) -> Option<usize> {
    for i in 0..ENTRIES {
        let e = &table[i * ENTRY..];
        for k in (0..3).rev() {
            let (a, b) = (e[14 + k] as i8, score[k] as i8);
            if a < b { return Some(i); }
            if a > b { break; }
        }
    }
    None
}

/// Shift the entries from `i` down one place (dropping the last) and put the score into entry `i`.
pub fn insert(table: &mut [u8], i: usize, score: [u8; 3]) {
    let tail = table[i * ENTRY..(ENTRIES - 1) * ENTRY].to_vec();
    table[(i + 1) * ENTRY..].copy_from_slice(&tail);
    table[i * ENTRY + 14..i * ENTRY + 17].copy_from_slice(&score);
}

pub fn set_name(table: &mut [u8], i: usize, name: &[u8]) {
    let e = &mut table[i * ENTRY..];
    let n = name.len().min(NAME_MAX);
    e[..n].copy_from_slice(&name[..n]);
    e[n] = b'$';
}

/// The BIOS 8x8 character set, read from Windows' CGA font file (the same IBM PC ROM glyphs).
pub struct Font { glyphs: Vec<[u8; 8]> }

impl Font {
    pub fn load() -> Option<Font> {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:/Windows".into());
        let d = std::fs::read(PathBuf::from(windir).join("Fonts").join("cga80woa.fon")).ok()?;
        Font::parse_fon(&d)
    }

    /// A Windows .fon (NE executable) with one FNT v2 8x8 font resource.
    fn parse_fon(d: &[u8]) -> Option<Font> {
        let u16at = |o: usize| -> Option<usize> { Some(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]) as usize) };
        let ne = u32::from_le_bytes(d.get(0x3c..0x40)?.try_into().ok()?) as usize;
        let rt = ne + u16at(ne + 0x24)?;
        let shift = u16at(rt)?;
        let mut p = rt + 2;
        loop {
            let (tid, cnt) = (u16at(p)?, u16at(p + 2)?);
            p += 8;
            if tid == 0 { return None; }
            for _ in 0..cnt {
                let (off, len) = (u16at(p)? << shift, u16at(p + 2)? << shift);
                p += 12;
                if tid != 0x8008 { continue; }
                let f = d.get(off..off + len)?;
                let fu16 = |o: usize| -> Option<usize> { Some(u16::from_le_bytes([*f.get(o)?, *f.get(o + 1)?]) as usize) };
                if fu16(0)? != 0x200 || fu16(0x56)? != 8 || fu16(0x58)? != 8 { continue; }
                let (first, last) = (*f.get(0x5f)? as usize, *f.get(0x60)? as usize);
                let mut glyphs = vec![[0u8; 8]; 256];
                for c in first..=last {
                    let go = fu16(0x76 + (c - first) * 4 + 2)?;
                    glyphs[c].copy_from_slice(f.get(go..go + 8)?);
                }
                return Some(Font { glyphs });
            }
        }
    }
}

/// A mode 0Dh screen with a BIOS text cursor.
pub struct TextScreen { pub img: Image, pub row: usize, pub col: usize }

impl TextScreen {
    pub fn new(background: &Image) -> TextScreen { TextScreen { img: Image { w: background.w, h: background.h, rgba: background.rgba.clone() }, row: 0, col: 0 } }

    pub fn cursor(&mut self, row: usize, col: usize) { self.row = row; self.col = col; }

    /// INT 10h AH=0Eh: draw the character cell and advance (CR/LF/BS move the cursor).
    pub fn teletype(&mut self, font: Option<&Font>, ch: u8, color: usize) {
        match ch {
            b'\r' => { self.col = 0; return; }
            b'\n' => { self.row = (self.row + 1).min(24); return; }
            8 => { self.col = self.col.saturating_sub(1); return; }
            _ => {}
        }
        let glyph = font.map_or([0; 8], |f| f.glyphs[ch as usize]);
        let rgb = EGA[color & 15];
        for (r, bits) in glyph.iter().enumerate() {
            for b in 0..8 {
                let (x, y) = (self.col * 8 + b, self.row * 8 + r);
                if x >= self.img.w || y >= self.img.h { continue; }
                let o = (y * self.img.w + x) * 4;
                let c = if bits >> (7 - b) & 1 == 1 { rgb } else { [0, 0, 0] };
                self.img.rgba[o..o + 3].copy_from_slice(&c);
                self.img.rgba[o + 3] = 255;
            }
        }
        self.col += 1;
        if self.col >= 40 { self.col = 0; self.row = (self.row + 1).min(24); }
    }

    /// Print a '$'-terminated string (0x233e).
    pub fn print(&mut self, font: Option<&Font>, s: &[u8], color: usize) {
        for &c in s.iter().take_while(|&&c| c != b'$') { self.teletype(font, c, color); }
    }

    /// The table itself (0x22ba): rows from 7 with a blank line after the first, name at column 8, score at 24.
    pub fn draw_table(&mut self, font: Option<&Font>, table: &[u8]) {
        let mut row = 7;
        for i in 0..ENTRIES {
            let e = &table[i * ENTRY..(i + 1) * ENTRY];
            self.cursor(row, 8);
            self.print(font, e, 0xb);
            self.cursor(row, 24);
            for k in (0..3).rev() {
                let v = e[14 + k] as i8 as i32;
                // 0x231a: tens by repeated subtraction (signed), then units
                let (mut t, mut c) = (0i32, v);
                loop { c -= 10; if c < 0 { break; } t += 1; }
                self.teletype(font, (t + 0x30) as u8, 0xc);
                self.teletype(font, (c + 10 + 0x30) as u8, 0xc);
            }
            self.print(font, b"00$", 0xc);
            row += if i == 0 { 2 } else { 1 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_and_insert() {
        let mut t = default_table();
        assert_eq!(rank(&t, [0, 0, 0]), None, "a zero score does not beat the empty table");
        let s = score_bytes(12345); // displayed 1234500
        assert_eq!(s, [45, 23, 1]);
        assert_eq!(rank(&t, s), Some(0));
        insert(&mut t, 0, s);
        set_name(&mut t, 0, b"COMIC");
        assert_eq!(&t[..6], b"COMIC$");
        assert_eq!(rank(&t, s), Some(1), "an equal score goes below");
        assert_eq!(rank(&t, score_bytes(20000)), Some(0));
        insert(&mut t, 0, score_bytes(20000));
        assert_eq!(&t[ENTRY..ENTRY + 6], b"COMIC$", "older entries move down");
        assert_eq!(t.len(), ENTRY * ENTRIES);
    }

    #[test]
    fn windows_cga_font() {
        let Some(f) = Font::load() else { return };
        assert_eq!(f.glyphs[b'A' as usize], [0x30, 0x78, 0xcc, 0xcc, 0xfc, 0xcc, 0xcc, 0x00]);
    }
}
