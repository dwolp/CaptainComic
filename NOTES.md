# Captain Comic reverse-engineering notes

Source: `AdvCapCo/comic.exe` (Revision 3, 1988-89, Michael Denio / Epyx), EXEPACK-compressed.
Unpacked with `work/unexepack.py` -> `work/comic_unpacked.exe`. Ghidra project in `work/ghidra/`
(needs JDK 25+; scripts `FixInt3.java`, `DumpAll.java`, `Seed.java`). Outputs in `work/out/`.

## Image layout (image offset = (segment - 0x1000) * 16)
- Code: CS=0x1000, offsets 0x20..0x234f (~3500 instrs, 99 funcs, fully disassembled, no gaps)
- Data: DS=0x1235 (image 0x2350). Other segments are used for level records (0x1f52) and text (0x1fba).
- Strings in the data segment are XOR 0x25.

## INT3 = game service dispatcher (handler at 1000:0558), NOT a debug trap
AL=0 sound on, 1 start tone (BX=ptr, CL=priority), 2 sound off, 3 stop, 4 query playing; other AL -> returns ~AL.
Ghidra needs the INT3 flow override (FixInt3.java) or it stops tracing.

## Control flow
entry 0x20 -> EGA check -> key config screen (0x174) -> intro (0x1de..) -> game main at 0x070c.
0x070c loads the level: copies the 203-byte level record, reads tt2 into DS:0x740 (4-byte header + tiles),
reads 3 maps (.pt) and sprite shp files, then 0x08b5 picks the stage, builds enemy slots, and jumps to the main loop at 0x1233.

## Map / collision
- Coordinates are in 8-px units. Map cell = (y/2)*128 + (x/2)... i.e. map row-major, 128 wide, 16x16 tiles.
- Tile id > byte0 of the .tt2 header (DS:0x740) is SOLID. (Function 0x0a34 / 0x09fa; 09fa also checks the neighbour tile when x is odd.)
- Map pointer in DS:0x4744, data starts after the 4-byte .pt header.

## Level records (image 0x9520 + level*0xd0, 203 bytes, 8 levels)
Level index order: 0 lake, 1 forest, 2 space, 3 base, 4 cave, 5 shed, 6 castle, 7 comp.
- 0x00,0x0e,0x1c,0x2a: 14-byte names tt2, pt0, pt1, pt2
- 0x38: 4 tile-id bytes (meaning TBD); 0x3c..: up to 4 sprite entries {frames/anim bytes + 12-byte shp name}
- 0x80..: per-stage tables (doors/exits at stage desc +5, 4 enemy slots at +0x11 as {shape idx, behaviour}) TBD
## Enemy slot (12 bytes at DS:0x5654): +4 hp?=0x14, +5 anim frame count, +6 behaviour(bit7 -> flag 2 at +0xb), +7 ptr to sprite data, +9/+a state.

## Game tick
INT 8 handler (0x4aa) sets tick flag every 2nd BIOS timer tick (18.2Hz) => ~9.1 game ticks/s (0.10985 s). Main loop 0x1233.
Keys (ISR 0x409 -> flags at CS:[0x13..0x18]); prompts in order: left, right, jump, fire, open door, teleport. F1-F4 -> flags CS:[0xf..0x12].

## Player (state in DS; y in 8px units "AL", x "AH" of word [0x40]; camera x = [0x42])
Size 2x4 units (16x32px). Frames at DS:0x70c0 (image 0x9410), stride 320: 4 planes x 64B (16x32), mask 64B at +0x100.
Frame index = facing(0 right / 5 left) + anim (0 stand,1-3 walk,4 jump/fall).
Vars: cf12 walk cycle 1..3, cf13 airborne, cf15 vx, cf16 vy, cf17 head-bump carry, cf18 jump counter (reload from [0x48]=4, boots=5), cf1a facing, cf1b anim.
Ground: ground tile = (y+4, x[,x+1 if x odd]); if not solid -> fall (vy=8, vx=+-2, cf18=1).
Jump: counter ticks while key held, vy-=7 each; y += vy>>3 (arith); gravity vy+=5 (space level 2: +3), terminal 23.
Air x: key held vx+-1 (clamp +-5), then |vx| decays by 1 and steps 1 unit/tick; ground walk = 1 unit/tick (turn costs a tick).
Side check: tile (y+3, x-1) going left, (y+3, x+2) going right. Land: tile (y+5,x[,x+1]); y=(y+1)&~1. Fall off when y>=17.
Camera: keeps player 10..12 units from left edge; clamp 0..232. Stage exits at x==0 (left) / x==254 (right).

## Stage descriptor (25 bytes, level record +0x80 + 0x19*stage)
+0 item type(0xff none) +1 item y +2 item x +3 left stage +4 right stage (0xff none)
+5..16 doors {y, x, dest level, dest stage} x3 (y=0xff none)  +17..24 enemies {sprite idx, behaviour} x4 (bit7 of behaviour = flag)
Enter door: open key at |door.x - px| in 0..2, same y, needs door key ([0x4a]). Arrive at the destination stage's door that points back (y, x+1).
Item collect: |item.x - px| < 2 and 0 <= item.y - py < 4. Collected flag at DS:0x52 + stage*16 + level.
Item types (sprites 16x16 at DS:0x9e84, 160B each: planes 128 + mask 32; 2 anim frames +0xa00):
0 corkscrew,1 door key ([0x4a]),2 boots ([0x48]=5),3 lantern ([0x4b]; castle is dark/blank tiles w/o it),4 teleport wand ([0x4c]),5 gems(extra life),6 crown,7 gold,8 cola,9-13 numbered tokens,0xe shield.
Start: level 1 (forest) stage 0, pos y=12 x=14. HUD vars: [0x44] lives(max5), [0x45] hp pips(max 6), [0x47] fire energy 0..12, score = base-100 digits [0x4e..0x50].

## Legal
Title screen: shareware, redistributable, but "program and graphics may not be distributed in any modified form",
no compensation. => Never ship the original assets with any modified build; the Rust engine must load the user's own files.

## Enemies (function 0x16cf, slots DS:0x5654, 12 bytes each, 4 per stage)
Slot: +0 y, +1 x, +2 dx(1/-1), +3 vy, +4 anim frame (or respawn timer when inactive), +5 frame count, +6 behaviour, +7 sprite table ptr,
+9 state (0 inactive, 1 alive, 2..6 exploding by fireball, 8..12 exploding on contact, 7 = despawn next tick), +a frame-set offset (5 when moving right), +b speed toggle.
Descriptor byte {sprite idx, behaviour}: behaviour bit7 = fast (toggle starts 2; normal speed moves every other tick); 0x7f = disabled slot.
Spawn (inactive, timer underflow; max 1 per tick): x = cam + off (facing right; off cycles 2,4..30 then 24..30; signed-overflow quirk) or cam-(off-22) (facing left),
y = scan upward from (player.y&~1)+4 through solid tiles to the first empty one; needs solid ground at that column. Despawn when |x-px|>=30: respawn timer cycles 0,20,40..100.
Behaviours: 1 diagonal bouncer (turns at walls / screen edges / floor / ceiling), 2 hopper (leaps toward player, gravity +2, terminal 23),
3 ground walker chasing player (falls 1/tick off ledges), 4 flying chaser (horizontal first, else vertical), 5 bouncer that flees upward when the player faces it, otherwise matches the player's row.
Contact: |dx|<2 and 0<=enemy.y-player.y<4 -> enemy explodes, hp-1; if hp==0 the player dies instead.
Fireball (slot DS:0xcf70, 6 bytes, up to [0x46] <= 5 slots, +1 per Blastola Cola): spawns at (y+1,x), dx=+-2/tick, corkscrew item adds y zigzag; hit if 0<=fy-ey<=1 and |dx|<=1; +3 score.
Fire meter [0x47] 0..12: key held costs 1 per 2 ticks, recharges +1 per 2 ticks when not firing (nothing happens at 0 while held).
Shp sprite frame = 4 planes (128B) then mask (32B). Level record sprite entries (0x3c + 0x11*k): {frames, mode (2 = second half is the other direction), extra (1 = append frame n-2), name[14]}.
Explosion frames: pointer table DS:0x9e64 (5, 16x16, mask +0x80); fireball DS:0x9e60 (2, 16x8, mask +0x40); teleport puffs DS:0x9e7a (5, 16x32, mask +0x100).
HP: 0..6 pips; lives 4 spare (max 5); refill 1/tick for 6 ticks after death/shield; shield at full hp -> extra life. Treasures (gems/crown/gold) = extra life; all 3 -> win.
Score +20 per item; every 500 points an extra life. Teleport wand (0x0eac): 6 units forward/back, needs a solid column with 2 empty rows above; 6-tick animation.

Palette: the game sets palette regs 10/12 to 0x1a/0x1c, but in 200-line mode 0Dh the monitor reads values as CGA RGBI (bit4 = intensity, bits 3/5 ignored), so the result is the plain 16-colour palette (light green 55ff55, light red ff5555). Confirmed against a DOSBox screenshot.

## Title / opening / ending / game over / HUD (all decoded)
Screens (sys00N.ega, 320x200): 000 title, 001 "Episode I" story, 002 ending text ("Episode II: Fractured Reality"), 003 in-game window frame, 004 item/enemy legend, 005 "Famous Comics" frame (unused here).
Title flow (0x0209..0x0324): title sys000 shown with palette regs 2/10/12 black, title tune starts, fade-in (0x0340: regs 2/10/12 -> 0x18, 7, (7,0x1f,0x1f), (2,0x1a,0x1f), then 0x1c; 1 tick each), 10 more ticks, no key check -> story sys001 with the same fade, keyboard buffer flushed, wait key -> legend sys004, wait key -> the bare sys003 frame ("The Planet Tambi" picture in the playfield), sound off, 5 life icons light up 1/tick (0x1d3e), 3 ticks, one life spent (0x1d81), start jingle, level load (0x070c). sys005 = "Famous Comics" high-score table, shown after game over (0x2190, file at DS:0xcff0) (ported).
Pause (0x1383 in the player routine, every tick except teleport): Esc held -> map only (0x0ad4) + banner DS:0xb5c4 "Game Paused / Press 'Q' to Quit" at the game-over banner spot, BIOS key buffer flushed, wait key: q/Q exits to DOS, other key resumes once Esc is released.
Game end (0x101e game over / 0x10ec win): win = beam out, score ticks, spare lives cashed in, title tune + sys002 story with fade (5 ticks) + 20 ticks, key, map + game-over banner; both then flush + wait key (0x104a) -> high scores (0x2190) -> exit to DOS (0x03d9, silent: exit string at 0x6fa is empty).
High scores (0x2190): mode 0Dh reset, sys005 on page 0, COMIC.HGH (10 x 18 bytes: name '$'-terminated max 12, score 3 base-100 bytes at +14 LSB first; missing -> 13 spaces + '$', score 0). First entry with lower score (MSB first, signed, ties don't displace) -> shift down, "Great Score!" (row 10 col 14, colour 9), "Enter Your Name: " (row 11 col 5, colour 10), DOS 0Ah input (13-byte buffer), erase both rows with 30 spaces (colours 1/2), write file. Table: rows 7, 9..17 (blank line after #1), name col 8 colour 11, score col 24 as 3 two-digit groups + "00" colour 12. BIOS teletype in planar mode paints the full 8x8 cell (background black). Port: hiscore.rs, BIOS 8x8 font read from Windows' cga80woa.fon.
Opening (0x11b1): 15 empty ticks (playfield + item only), sound wait, then 12 beam frames (DS:0x8f60, stride 320, 16x32 + mask +0x100) with Comic drawn from frame 6 on, then 6 ticks.
Death (0x0e5c): 8 frames at DS:0x81a0, Comic drawn for the first 4; enemies keep animating. Falling off the bottom also kills.
Game over (0x101e): banner 128x48 at DS:0xc1c4 (4 planes, opaque) drawn at playfield px (32,56); any key then ends.
Ending (0x109d): 12 beam frames (Comic drawn for the first 6) + 6 ticks, +10 score x20 ticks, then per spare life +10 x10 ticks, life-1, 3 ticks; then sys002 until key, then the banner.
Window layout (virtual 320x200): playfield at (8,8) size 192x160. Score = score*100, 8 digits at cols 29..36 x=232.., y=24 (digit sprites DS:0xb284+d*64, 8x16 opaque).
HP pips j=1..6 at x=(30+j)*8, y=82 (empty DS:0xb504, half 0xb544, full 0xb584). Fire meter cells at y=54 (2 steps per cell). Lives icons at x=(3+3i)*8, y=180 (DS:0xa7e4, off = +0xa00, 16x16 opaque).
Inventory slots: corkscrew (256,112) key (280,112) boots (232,136) lantern (256,136) wand (280,136); gems/crown/gold at (232/256/280,160); cola count icon = item sprite 8+n at (232,112).

## Sound (decoded; INT3 service player in the INT 8 handler 0x4aa)
Sequences live in DS as {u16 PIT divisor, u16 duration in 18.2065 Hz ticks}... terminated by divisor 0; frequency = 1193182/divisor; divisor 40 = inaudible rest.
Service 1 (BX=ptr, CL=priority): ignored while a higher-priority sound plays. F1 = sound on, F2 = off (flags CS:[0xf]/[0x10]).
DS addr (prio): 0x0082 title/ending-story tune (4, ~13.7s), 0x06a2 game start (1), 0xcde9 door (4), 0xce11 death (4), 0xce39 teleport (2), 0xce59 score tick (3),
0xce65 beam in/out (4), 0xcec5 game over (2), 0xcf1f stage change (4), 0xcf3f fall off bottom (4), 0xcf8e fire (0), 0xcf9a enemy hit (1), 0xcfb1 hurt (2), 0xcfc1 item (3), 0xcfd5 extra life (4).
Rust: sound.rs renders them as 22.05 kHz square-wave WAVs; main.rs Audio applies the priority rule. Needs macroquad "audio" feature (enabled).
Caveat: the dev machine had no audio endpoint (quad-snd panics in its thread, game continues), so playback itself was never heard here. CC_NO_AUDIO=1 skips playing sounds (macroquad still starts its audio thread, so a machine without an audio device prints one harmless panic message from that thread).

## Door animation and the opening wait (decoded 2026-11-20)
Door out (0x0d25, DoorAnim in game.rs): 4 one-tick frames, only the map is drawn (no enemies/items): door area = 4x4 units at the door's (y,x), cleared to black (0x0e1e),
posts = right half of tile ids t0/t2 at x+0 and left half of t1/t3 at x+24px (rows 0 and 16px; ids are the 4 bytes at level record +0x38, DS:0x7010), then Comic.
Frames: [clear,posts,player] [clear,player] [posts,clear,player,posts] [map only]. Then the stage loads, 3 ticks wait on the old scene, door sound, and arriving (0x0ca8): 5 frames
[map] [clear,player,posts] [clear,player] [posts,clear,posts,player] [player]. Total 4 + 3 + 5 ticks. The door sound (0xcde9) plays leaving and arriving.
Opening (0x11b1): 15 empty ticks, then it waits while ANY sound is playing (INT3 service 4; the start jingle is 131 timer ticks = ~66 game ticks), then beam sound + 12 beam frames + 6 ticks.
Sound priority/timing now lives in Game (speaker model: remaining 18.2 Hz ticks, 2 per game tick); the host just plays what the game accepts.

## Falling off the bottom (0x142f, decoded) / hurt feedback
Phase::Fall: 1 tick with Comic sinking out of the bottom in the jump pose (original draws only the top (20-y)*8 sprite rows; the playfield clip does the same), 2 ticks of empty playfield,
fall sound (0xcf3f), 15 more ticks, then the game-over check / life lost / respawn. Enemies and input are frozen throughout. The original has NO red hurt flash (removed from the port).

## Differential testing against the original (work/emu)
`orig.py` runs the unpacked original comic.exe in a Unicorn x86-16 emulator: DOS file calls served from AdvCapCo, BIOS/EGA calls stubbed,
timer IRQ 0 delivered by the harness (2 per game tick), keys injected into CS:[0x13..0x18], a HLT patched into the main-loop top (0x1233) so each
logic tick ends exactly there. The idle-loop jump-counter reload (0x125b) is NOPed and done by the harness (sees the next tick's keys, like real play).
`captain-comic/src/trace.rs` (bin `trace`) prints the same state per tick from the Rust port. `compare.py` diffs the two; `fuzz.py <levels> <seeds> <ticks>`
runs random input scripts (with/without cheat items). Result: 96 scripts x 800 ticks over all 8 levels are identical tick for tick
(pos, cam, vx/vy, air, hp, lives, meter, score, enemy slots + internals, fireballs, spawn counters, jump counter, walk cycle).
Not compared: sound, pixels, game-over/ending screens, win. Needs `pip install unicorn`. DOSBox Staging is installed too (work/dosbox/test.conf) but
could not be driven here (no interactive desktop: SendKeys "Access is denied"). Door phases are run to completion by trace.rs, like the death animation.

### Original quirks the port reproduces (found by the diff)
- Initial data: DS:0xcf60 = 0x14 (respawn timer), 0xcf61 = 0x16 (spawn offset), HP starts 0 and refills 1/tick (cf1c=6).
- 0x0a34 (enemy side-collision) checks the tile BELOW too when y is odd; 0x09fa checks the next column when x is odd.
- Enemy spawn search: scan up from (py&~1)+4 through empty tiles to the first solid, then up through solid to the first empty; x = (cam+off)&0xff wraps near the right edge.
- Teleport right (0x0ef8) compares a byte against 0xe8 with a signed JLE: camera < 128 always scrolls ~6 units; camera is never clamped during it. A failed teleport still scrolls.
- Fireball/enemy hit uses a signed compare on |dx|: distances >= 128 count as a hit (wrapped x=0 enemy gets hit by a fireball at x~242).
- Pressing teleport skips walking/firing in that tick and starts the animation step in the same tick. Idle-loop reload of the jump counter also runs during animations.
- Leaving a stage by walking off an edge zeroes vy but keeps the air flag and all other motion state; only death resets it. Falling off the bottom is instant (no enemy ticks); enemy-kill death runs 8 enemy passes.
- Behaviour 3 clears its speed toggle when dx==0; behaviour 5 writes [+3] (unused). New spawn resets the frame/timer byte to 0.


## Rust port status (captain-comic/)
The original game logic is complete and tested (exe.rs, data.rs, assets.rs, game.rs, sound.rs, input.rs, hiscore.rs; main.rs is the host: screens, loop, drawing). Unit tests plus differential tests against the original (`cargo test --release`). In the faithful settings the screen changes only on a game tick (~9.1 Hz, wall-clock paced) with the DOS 1.2x tall pixels, like the original.
Run: `cargo run --release -- ../AdvCapCo`, or `play.bat` (builds and starts `CaptainComic.exe`). Test hooks (environment): `CC_SCREEN=play|story|legend`, `CC_LEVEL=n`, `CC_CHEAT`, `CC_HOLD=left,jump,...`, `CC_FAST=n`, `CC_SHOT_AFTER=n` + `CC_SHOT_FILE=x.png`, `CC_ENDING`, `CC_GAMEOVER`, `CC_MENU`, `CC_WIDE`, `CC_CRT`, `CC_EFFECTS`, `CC_IMMERSIVE`, `CC_INFFIRE`, `CC_SHOWHINT`, `CC_PAUSE`, `CC_DOORSHOTS`, `CC_FALLSHOTS`, `CC_SQUARE`, `CC_NO_AUDIO`.

## TODO
1. Listen-test sound on a machine with an audio device (work/sound_title.wav, sound_item.wav are reference renders).
2. Pixel-level comparison with a real DOSBox screenshot / the original's EGA memory.
3. Confirmed working by the user in real play: rumble (stops), controller remapping, the copyright screen with a pad, volume, Immersive at 16:9 fullscreen, Load game from the title, the stage iris.

## Single exe
`cargo build --release` gives one self-contained exe (static CRT via `captain-comic/.cargo/config.toml`). It looks for the original game in a `game` folder next to it (a zip is unpacked once). On start it switches to its own folder, so `comic.cfg`, `comic.sav`, `KEYS.DEF` and `COMIC.HGH` are written next to the exe whatever the start directory. `play.bat` copies the build to `CaptainComic.exe` in the project root. The original game files are never bundled (the licence forbids distributing modified program or graphics).

## Optional enhancements (current state)
Everything is optional and off by default (the faithful behaviour is untouched), except the window: with no `comic.cfg` the game starts fullscreen with Fit scaling (`Settings::default`). Settings live in `comic.cfg` (`key=value`, `settings.rs`): `smooth`, `scaling` (integer|fit), `fullscreen`, `hints`, `widescreen`, `crt`, `effects`, `weather` (off|rain), `immersive`, `hud` (0..2), `rumble`, `cheat_fire`, `cheat_health`, `volume` (0..10), `pad_jump|pad_fire|pad_open|pad_teleport`. Unknown or removed keys (`aspect`, `pixels`, `bloom`) are ignored. The cheats are saved too (`cheat_fire`, `cheat_health`). With infinite health on, the original's "refill at full health pays 0x12 points per tick" (every death by falling) is suppressed so the cheat cannot inflate the score; without the cheat it is the original behaviour.

**F1 menu** (`menu.rs`, opens on Title/Story/Legend/Play; Start on the pad too; Esc, F1, Start or B close it). Rows: Enhanced, Scaling, Aspect ratio (4:3 / 16:9), Window frame (Classic / Immersive), HUD size, Fullscreen, CRT filter, Weather, Volume, Infinite fireballs, Infinite health, Remap keys, Remap controller, Save game, Load game, Restart game (confirm), Quit game (confirm). The right panel lists keys and controller buttons side by side. Other keys: F2 sound, F5 / F9 save / load, F11 fullscreen, Select on the pad = Enhanced on/off.

**Enhanced** (first row) = smooth motion, effects, controller rumble and the locked-door hint (`Settings::enhanced_count`, `ENHANCED_TOTAL = 4`, `set_enhanced`). On when all are on, Off when none, Custom otherwise. These four have no rows of their own. Widescreen is chosen with the Aspect ratio row and is not part of it.

- **Save/load**: `comic.sav` (working directory): a checkpoint (level, stage, position, hp, lives, score, fire-ball upgrades, items, treasures, collected). Enemies start fresh. Only while `Game::can_save()`.
- **Smooth motion**: `Snap` keeps the previous tick's positions (Comic, camera, enemies, fireballs); the draw interpolates by `acc / TICK_SECS`. Jumps over 8 units, doors, deaths, teleports and stage changes are not smoothed. Enemies glide toward their position via `vis` (~70 ms lag).
- **Locked-door hint**: `Game::hint` after pressing Open at a door without the key; drawn with the BIOS 8x8 font (`Sprites::hint`). Toasts ("Game saved", ...) use Consolas, 36 px, with a shadow.
- **Gamepad** (`pad.rs`, gilrs): d-pad / left stick move, A jump, X fire, B or d-pad down open door, Y teleport (jump/fire/open/teleport remappable in "Remap controller"; Start, Select, d-pad stay reserved). Any button gets past the copyright screen. High-score name entry works with the pad (`HighScore::input`, `PAD_CHARS`): d-pad up/down picks a letter (A-Z, 0-9, space, . -), A or right adds it, B or left deletes, Start finishes (a picked letter that was not added yet still counts); the keyboard works as before, and any button leaves the final table. A hint is shown when a controller is connected. Backup before: `work/backups/src_pre_padname`. Rumble (`Pad::rumble`) on damage (short) and on death or falling (long); the effect has an explicit end because gilrs repeats forever by default.
- **Volume**: 0-100% in steps of 10 (50% = the original loudness); F2 mutes.
- **Widescreen** (16:9): the canvas grows from 320 to 448 px (`WIDE_EXTRA`); `widen_frame` repeats one column of the frame so the panel keeps its shape; the view is 40 units and follows the camera, clamped to the map ends; `Game::view_margin` lets fireballs fly further. Enemies still spawn at the original window's edge, so they appear inside the wider view. With Effects on, the spawn fade-in and puff make this look like enemies arriving from another dimension, which the user likes: deliberately left as is. A 1px black outline after the frame hides a stray magenta frame pixel at fractional scales.
- **CRT filter**: the picture is drawn into a 4x render target and through a GLSL shader (`CRT_FRAGMENT`: curvature, bleed, glow, scanlines (`Lines` uniform), phosphor mask, vignette). The menu and toasts are drawn afterwards, unfiltered. Not used on the DOS text screens. It is the only post filter (pixel smoothing and bloom were tried and removed).
- **Immersive window**: no frame; the playfield (160 units tall, as wide as the window shows, `ex` a whole number) fills the window; the CRT filter works on it too. HUD (`draw_overlay_hud`): score top right, shield bar top left, lives bottom left, items bottom right on a translucent grey box; every HUD sprite is drawn with a whole number of screen pixels per source pixel (HUD size = Small 0.5 / Medium 0.75 / Large 1.0 times the window scale), with the sprites' black/grey backgrounds made transparent (`ov_digits`, `ov_pips`, `ov_life`). Integer scaling is ignored there; the menu, title and story keep the classic layout.
- **Effects** (`fx.rs`; `Fx::observe` compares the `Snap` taken before a tick with the state after it, `Fx::update` runs per frame; nothing feeds back into the game):
  - Particles for kills, landings, damage, pickups; glow around fireballs and the stage item; soft shadow under Comic; red flash and shake on damage, death and falling out of the screen; the score counts up; enemies fade in with a puff when they spawn.
  - Fireballs: muzzle flash and embers, a few sparks while flying, flash and burst where they end (bigger on a kill). Doors: golden sparks and a warm light spilling from the doorway.
  - Stage arrival: the old fade is an iris opening around Comic.
  - Ambient (`draw_ambient`, by level index: 0 lake, 1 forest, 2 space, 3 base, 4 cave, 5 shed, 6 castle, 7 comp): lake water glints; space star twinkle; forest fireflies and drifting ground mist (`draw_mist`, moving sideways, bobbing and pulsing); castle torch/brazier light, and once the Lantern is found the castle is dimmed except for a clear disc around Comic (`darkness`; the pre-Lantern pitch black is untouched); cave torches, sparkling damp walls and drips; base console lights on the lit console pixels and star twinkle on the black window pixels (`fx::Spots`, per-tile pixel lists). Tile ids were read off the tileset sheets, so a few decorative tiles may be missed. Shed (level 5, tile ids from its tileset sheet): water glints on tiles 4/10/11, a pulse running along the red pipe tiles 21..=23 (`Fx::pulse` over the tile's lit pixels, a wave on the world pixel diagonal), glow plus a sweeping highlight on the coloured block tiles 24 and 28..=31 (`Fx::sweep`). Computer (level 7): data pulses along the red/green circuit-line tiles 0, 1 and 6..=13, pulsing glow on the disc tiles 41..=50, a rolling bright line on the magenta panel tiles 2..=5. `fx::Spots` is now built for the base, shed and computer tilesets. Only the computer pulses and the shed's door-water glints have been seen in screenshots; the pipe and block tiles did not appear in the stages tried.
  - Weather (`draw_weather`, own menu row, needs Effects): rain over the lake and forest.

## Backups
There is no git repository. `work/backups/src_*` holds copies of the sources; `src_2026-11-23` is the latest. Copy `captain-comic/src` there before risky bulk edits (main.rs was once truncated by a bad patch script and had to be rebuilt from the session transcript). The `src_pre_*` folders are the state just before each enhancement step and can be deleted when no longer wanted.
- The release exe is a Windows-subsystem program (`windows_subsystem = "windows"` in main.rs, not for debug builds or tests), so no console window opens behind the game; `play.bat` starts it with `start` so its own window closes too.
