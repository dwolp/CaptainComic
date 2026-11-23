# Captain Comic

A native Windows port of the 1988 DOS game *The Adventures of Captain Comic* (by Michael Denio), written in Rust with
[macroquad](https://macroquad.rs). The game logic was reverse engineered from the original program and is checked
against it with differential tests (see `NOTES.md`). By default it plays like the original; every enhancement is
optional.

**This repository contains no original game files.** You need your own copy of the game: put the original zip (for
example `Adventures of Captain Comic, The (1988).zip`) or the unpacked folder that contains `comic.exe` in a folder
named `game` next to the executable, or point the program at a folder or `.zip` on the command line. A zip is
unpacked once next to itself. The original's licence does not allow redistributing it, so please do not add it here.

## Build and run

```
cd captain-comic
cargo build --release
```

The result is a single executable (`captain-comic/target/release/captain-comic.exe`, static runtime). On Windows,
`play.bat` builds it, copies it to `CaptainComic.exe` in the project folder and starts it. Settings, the save file,
key bindings and high scores are written next to the executable.

## Playing

Default keys are the original's: arrows move, Space jumps, Insert fires, Alt opens doors, Caps Lock teleports.
A gamepad works too (d-pad or stick to move, A jump, X fire, B or d-pad down open a door, Y teleport).

**F1** (or Start on a pad) opens the settings menu:

- **Enhanced** turns on smooth 60 fps motion, visual effects (particles, glow, ambient details in each level),
  controller rumble and a hint at locked doors. Select on a pad toggles it.
- Aspect ratio (4:3 or 16:9), an **Immersive** mode without the window frame, HUD size, fullscreen, scaling, an
  optional CRT filter, rain, volume.
- Save and load game (also F5 / F9), key and controller remapping, infinite fireballs and health, restart, quit.

Everything is off by default, so the faithful original behaviour is what you get until you change something (apart
from starting fullscreen).

## Notes

`NOTES.md` documents how the original works (map and collision rules, enemy behaviours, sound, level data) and how
the port is organised. `cargo test --release` runs the unit tests; the tests that compare against the original
program are skipped when the original files are not found at `../AdvCapCo`.

## Licence

The code in this repository is released under the [MIT licence](LICENSE). That covers only this port. *The
Adventures of Captain Comic* itself, its graphics and its sound remain the property of their owners and are not part
of this repository.
