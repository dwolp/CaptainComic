//! Reads the original `comic.exe` (Microsoft EXEPACK-compressed) and exposes its memory image.
//! Image offset = (segment - 0x1000) * 16 + offset; the game's data segment (DS) is 0x1235.
use std::{fs, io, path::Path};

pub struct Exe {
    img: Vec<u8>,
}

const DS_BASE: usize = 0x2350; // (0x1235 - 0x1000) * 16

impl Exe {
    pub fn load(path: &Path) -> io::Result<Exe> {
        let d = fs::read(path)?;
        Self::unpack(&d).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a supported EXEPACK comic.exe"))
    }

    fn unpack(d: &[u8]) -> Option<Exe> {
        let w = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]) as usize;
        if d.len() < 0x20 || &d[..2] != b"MZ" { return None; }
        let size = (w(4) - 1) * 512 + w(2);
        let hdr = w(8) * 16;
        let cs = w(0x16);
        let h = hdr + cs * 16; // EXEPACK header (18 bytes) lives at CS:0
        if h + 18 > d.len() || &d[h + 16..h + 18] != b"RB" { return None; }
        let dest_len = w(h + 12) * 16;
        let data = &d[hdr..h.min(size)];
        let mut end = data.len();
        while end > 0 && data[end - 1] == 0xFF { end -= 1; }
        let mut out: Vec<u8> = Vec::with_capacity(dest_len); // built backwards
        let mut pos = end;
        loop {
            if pos < 3 { return None; }
            let op = data[pos - 1];
            let len = data[pos - 3] as usize | (data[pos - 2] as usize) << 8;
            pos -= 3;
            match op & 0xFE {
                0xB0 => { let b = *data.get(pos.checked_sub(1)?)?; out.extend(std::iter::repeat(b).take(len)); pos -= 1; }
                0xB2 => { let s = pos.checked_sub(len)?; out.extend(data[s..pos].iter().rev()); pos -= len; }
                _ => return None,
            }
            if op & 1 != 0 { break; }
        }
        let mut img = data[..pos].to_vec();
        out.reverse();
        img.extend(out);
        if img.len() != dest_len { return None; }
        Some(Exe { img })
    }

    /// Bytes in the main data segment (DS = 0x1235).
    pub fn ds(&self, off: usize, len: usize) -> &[u8] { &self.img[DS_BASE + off..DS_BASE + off + len] }
    /// Bytes at an arbitrary segment:offset.
    pub fn seg(&self, seg: usize, off: usize, len: usize) -> &[u8] {
        let o = (seg - 0x1000) * 16 + off;
        &self.img[o..o + len]
    }
}
