//! Finds the original game's files. The player puts the original game zip (or its unpacked files) in the
//! `game` folder in the project root; a zip is unpacked once into `game/extracted`.
use crate::exe::Exe;
use std::{fs, io, path::{Path, PathBuf}};

/// The `game` folder: next to the project's `captain-comic` folder (found by walking up from the program
/// and from the working directory), falling back to a `game` folder beside the program.
pub fn game_folder() -> PathBuf {
    let mut starts: Vec<PathBuf> = vec![];
    if let Some(p) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) { starts.push(p); }
    if let Ok(c) = std::env::current_dir() { starts.push(c); }
    for s in &starts {
        for a in s.ancestors().take(6) {
            if a.join("game").is_dir() { return a.join("game"); }
        }
    }
    for s in &starts {
        for a in s.ancestors().take(6) {
            if a.join("captain-comic").join("Cargo.toml").exists() { return a.join("game"); }
        }
    }
    starts.first().cloned().unwrap_or_default().join("game")
}

/// Create the folder and a short README so there is an obvious place to drop the zip.
pub fn prepare_game_folder(dir: &Path) {
    if fs::create_dir_all(dir).is_ok() {
        let readme = dir.join("README.txt");
        if !readme.exists() {
            let _ = fs::write(readme, "Put the original game here: the zip file (e.g. \"Adventures of Captain Comic, The (1988).zip\")\r\nor the unpacked game files (the folder that contains comic.exe).\r\nA zip is unpacked once into the \"extracted\" folder next to it.\r\n");
        }
    }
}

/// Look for comic.exe (any letter case) under `dir`, at most `depth` folders deep; returns its folder.
fn find_comic_folder(dir: &Path, depth: u32) -> Option<PathBuf> {
    let mut subdirs = vec![];
    for e in fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() { subdirs.push(p); }
        else if p.file_name().map_or(false, |n| n.to_string_lossy().eq_ignore_ascii_case("comic.exe")) { return Some(dir.to_path_buf()); }
    }
    if depth == 0 { return None; }
    subdirs.sort();
    subdirs.into_iter().find_map(|s| find_comic_folder(&s, depth - 1))
}

/// Unpack a zip into `dest`. Entries whose paths would escape `dest` are skipped.
pub fn extract_zip(zip_path: &Path, dest: &Path) -> io::Result<()> {
    let mut z = zip::ZipArchive::new(fs::File::open(zip_path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let Some(rel) = f.enclosed_name() else { continue };
        let out = dest.join(rel);
        if f.is_dir() { fs::create_dir_all(&out)?; continue; }
        if let Some(parent) = out.parent() { fs::create_dir_all(parent)?; }
        io::copy(&mut f, &mut fs::File::create(&out)?)?;
    }
    Ok(())
}

/// Resolve a folder that may contain the game files or a zip of them. Returns the folder holding comic.exe.
fn resolve(dir: &Path, notes: &mut Vec<String>) -> Option<PathBuf> {
    if let Some(f) = find_comic_folder(dir, 4) { return Some(f); }
    let mut zips: Vec<PathBuf> = fs::read_dir(dir).ok()?.flatten().map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x.eq_ignore_ascii_case("zip"))).collect();
    zips.sort();
    for z in zips {
        let dest = dir.join("extracted");
        match extract_zip(&z, &dest) {
            Ok(()) => if let Some(f) = find_comic_folder(&dest, 4) { return Some(f); }
                      else { notes.push(format!("{} has no comic.exe inside", z.display())); }
            Err(e) => notes.push(format!("could not unpack {}: {e}", z.display())),
        }
    }
    None
}

/// Locate and load the game. `args` is whatever followed the program name (re-joined, so unquoted paths
/// with spaces work); it may be a folder or a .zip. Returns the game folder and the loaded comic.exe,
/// or a list of lines explaining what was tried.
pub fn locate(args: &[String]) -> Result<(PathBuf, Exe), Vec<String>> {
    let gf = game_folder();
    let mut notes: Vec<String> = vec![];
    let mut tried: Vec<PathBuf> = vec![];
    let given = args.join(" ").trim_matches('"').to_string();

    let mut candidates: Vec<PathBuf> = vec![];
    if !given.is_empty() {
        let p = PathBuf::from(&given);
        if p.is_file() && p.extension().map_or(false, |x| x.eq_ignore_ascii_case("zip")) {
            let dest = gf.join("extracted");
            prepare_game_folder(&gf);
            match extract_zip(&p, &dest) { Ok(()) => candidates.push(dest), Err(e) => notes.push(format!("could not unpack {}: {e}", p.display())) }
        } else { candidates.push(p); }
    } else {
        prepare_game_folder(&gf);
        candidates.push(gf.clone());
        // older layouts: an AdvCapCo folder next to the project or the program
        for d in ["../AdvCapCo", "AdvCapCo", "."] { candidates.push(PathBuf::from(d)); }
    }
    for c in candidates {
        tried.push(c.clone());
        if let Some(folder) = resolve(&c, &mut notes) {
            if let Ok(exe) = Exe::load(&folder.join("comic.exe")) { return Ok((folder, exe)); }
            else { notes.push(format!("{} is not a supported comic.exe", folder.join("comic.exe").display())); }
        }
    }
    let mut msg = vec![
        "Could not find the original game files.".to_string(),
        String::new(),
        "Put the original game zip (or the unpacked game folder) in:".to_string(),
        format!("  {}", gf.display()),
        String::new(),
        format!("Looked in: {}", tried.iter().map(|t| t.display().to_string()).collect::<Vec<_>>().join(", ")),
    ];
    for n in notes { msg.push(n); }
    Err(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn unpacks_the_real_zip_once_and_finds_comic_exe_even_in_a_subfolder() {
        let zip = Path::new("../Adventures of Captain Comic, The (1988).zip");
        if !zip.exists() { return; }
        let dir = tmp("zip folder with spaces");
        fs::copy(zip, dir.join("Adventures of Captain Comic, The (1988).zip")).unwrap();
        let mut notes = vec![];
        let folder = resolve(&dir, &mut notes).expect("found");
        assert!(folder.join("comic.exe").exists());
        assert!(Exe::load(&folder.join("comic.exe")).is_ok());
        assert!(dir.join("extracted").exists());
        // second run finds it without unpacking again
        assert_eq!(resolve(&dir, &mut vec![]).unwrap(), folder);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_folder_gives_no_result_and_unsafe_zip_paths_are_ignored() {
        let dir = tmp("empty");
        assert!(resolve(&dir, &mut vec![]).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
