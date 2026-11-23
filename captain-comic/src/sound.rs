//! The original's PC-speaker sounds: sequences of {PIT divisor, duration in 18.2 Hz timer ticks}
//! stored in comic.exe's data segment, rendered here as square waves.
use crate::exe::Exe;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sound {
    Title, Start, Door, Death, Teleport, Bonus, Beam, GameOver, StageChange, Fall, Fire, EnemyHit, Hurt, Item, ExtraLife,
}

pub const ALL: [Sound; 15] = [
    Sound::Title, Sound::Start, Sound::Door, Sound::Death, Sound::Teleport, Sound::Bonus, Sound::Beam, Sound::GameOver,
    Sound::StageChange, Sound::Fall, Sound::Fire, Sound::EnemyHit, Sound::Hurt, Sound::Item, Sound::ExtraLife,
];

impl Sound {
    /// Address of the note list in the data segment (DS-relative).
    pub fn addr(self) -> usize {
        match self {
            Sound::Title => 0x0082, Sound::Start => 0x06a2, Sound::Door => 0xcde9, Sound::Death => 0xce11,
            Sound::Teleport => 0xce39, Sound::Bonus => 0xce59, Sound::Beam => 0xce65, Sound::GameOver => 0xcec5,
            Sound::StageChange => 0xcf1f, Sound::Fall => 0xcf3f, Sound::Fire => 0xcf8e, Sound::EnemyHit => 0xcf9a,
            Sound::Hurt => 0xcfb1, Sound::Item => 0xcfc1, Sound::ExtraLife => 0xcfd5,
        }
    }
    /// A sound only starts if nothing of higher priority is playing.
    pub fn priority(self) -> u8 {
        match self {
            Sound::Title | Sound::Door | Sound::Death | Sound::Beam | Sound::StageChange | Sound::Fall | Sound::ExtraLife => 4,
            Sound::Bonus | Sound::Item => 3,
            Sound::Teleport | Sound::GameOver | Sound::Hurt => 2,
            Sound::Start | Sound::EnemyHit => 1,
            Sound::Fire => 0,
        }
    }
}

pub const TICK_HZ: f64 = 1_193_182.0 / 65_536.0; // BIOS timer, 18.2065 Hz
const PIT_HZ: f64 = 1_193_182.0;

/// Read the (divisor, ticks) list for a sound.
pub fn sequence(exe: &Exe, s: Sound) -> Vec<(u16, u16)> {
    let mut out = vec![];
    let mut o = s.addr();
    while out.len() < 512 {
        let b = exe.ds(o, 4);
        let (div, dur) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
        if div == 0 { break; }
        out.push((div, dur));
        o += 4;
    }
    out
}

/// Length of a sound in 18.2 Hz timer ticks.
pub fn total_ticks(exe: &Exe, s: Sound) -> u32 { sequence(exe, s).iter().map(|n| n.1 as u32).sum() }

/// Render a sequence to a 16-bit mono WAV file in memory. Returns (wav bytes, duration in seconds).
pub fn synth(seq: &[(u16, u16)], rate: u32) -> (Vec<u8>, f32) {
    let mut samples: Vec<i16> = vec![];
    let mut phase = 0.0f64;
    for &(div, dur) in seq {
        let n = (dur as f64 / TICK_HZ * rate as f64).round() as usize;
        let freq = PIT_HZ / div as f64;
        for _ in 0..n {
            // frequencies above the audible range are the original's way of writing a rest
            let v = if freq > 9000.0 { 0.0 } else if phase < 0.5 { 1.0 } else { -1.0 };
            samples.push((v * 7000.0) as i16);
            phase = (phase + freq / rate as f64).fract();
        }
    }
    let secs = samples.len() as f32 / rate as f32;
    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for s in samples { wav.extend_from_slice(&s.to_le_bytes()); }
    (wav, secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn every_sound_decodes_and_renders() {
        let p = Path::new("../AdvCapCo/comic.exe");
        if !p.exists() { return; }
        let exe = Exe::load(p).unwrap();
        for s in ALL {
            let seq = sequence(&exe, s);
            assert!(!seq.is_empty(), "{s:?} has no notes");
            let ticks: u32 = seq.iter().map(|n| n.1 as u32).sum();
            let (wav, secs) = synth(&seq, 22050);
            assert_eq!(&wav[..4], b"RIFF");
            assert!((secs as f64 - ticks as f64 / TICK_HZ).abs() < 0.01, "{s:?}: {secs}s vs {ticks} ticks");
        }
        // the title tune is ~13.7 seconds long (249 ticks)
        let (_, secs) = synth(&sequence(&exe, Sound::Title), 22050);
        assert!((13.5..13.9).contains(&secs), "{secs}");
    }
}
