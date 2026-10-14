@echo off
rem Starts the browser server (docs\guides\hosting.md): builds whatever changed, then runs it.
cd /d "%~dp0"
tasklist /FI "IMAGENAME eq mc-rs-server.exe" 2>nul | find /I "mc-rs-server.exe" >nul
if not errorlevel 1 (
  echo An mc-rs-server is already running ^(its window, or one left from before^).
  choice /C YN /M "Stop it and start the current build"
  if errorlevel 2 goto :end
  taskkill /IM mc-rs-server.exe /F >nul
  timeout /t 1 /nobreak >nul
)
where cargo >nul 2>nul
if errorlevel 1 (
  if not exist "target\release\mc-rs-server.exe" (
    echo cargo is not on PATH and there is no build yet: install Rust from https://rustup.rs
    goto :end
  )
  echo cargo is not on PATH, so this runs the last build.
) else (
  echo Building what changed since the last start...
  cargo build --release -p viewer -p server || goto :end
)
target\release\mc-rs-server.exe %*
:end
pause
