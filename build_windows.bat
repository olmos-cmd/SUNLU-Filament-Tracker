@echo off
setlocal
where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust/Cargo wurde nicht gefunden.
  echo Bitte zuerst Rust von https://rustup.rs installieren.
  pause
  exit /b 1
)

echo Erstelle Windows-EXE...
cargo build --release
if errorlevel 1 (
  echo.
  echo Der Build ist fehlgeschlagen. Die Fehlermeldung steht oben.
  pause
  exit /b 1
)

echo.
echo Fertig: target\release\sunlu_filament_tracker.exe
pause
