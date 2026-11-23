@echo off
rem Builds the single exe (first time only) and starts it. The exe finds the original game in the "game" folder next to it.
cd /d "%~dp0"
if not exist CaptainComic.exe (
  pushd captain-comic
  cargo build --release
  popd
  copy /y captain-comic\target\release\captain-comic.exe CaptainComic.exe
)
CaptainComic.exe
