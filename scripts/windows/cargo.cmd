@echo off
rem Build GatedSpark on Windows with the GNU toolchain.
rem
rem Why this file exists: the pinned toolchain in rust-toolchain.toml selects the
rem "msvc" build on Windows, and that one has no linker of its own -- it borrows
rem link.exe from Visual Studio or the Microsoft Build Tools. On a machine without
rem those, every build fails with "linker `link.exe` not found". The GNU toolchain
rem ships its own linker, so it builds on a stock Windows machine with nothing but
rem rustup installed. Decided 2026-10-22; see docs/adr/0005-windows-gnu-toolchain.md.
rem
rem Usage, from anywhere in the repository:
rem     scripts\windows\cargo.cmd test --workspace
rem     scripts\windows\cargo.cmd clippy --all-targets
rem Everything after the script name is handed to cargo unchanged.
rem
rem rust-toolchain.toml stays the single place the version is written down. This
rem script reads it rather than repeating it, so the two can never drift apart.

setlocal

set "TOOLCHAIN_FILE=%~dp0..\..\rust-toolchain.toml"
if not exist "%TOOLCHAIN_FILE%" (
  echo [gatedspark] cannot find rust-toolchain.toml next to this script.
  exit /b 1
)

set "CHANNEL="
for /f "usebackq tokens=2 delims==" %%A in (`findstr /r /c:"^channel *=" "%TOOLCHAIN_FILE%"`) do (
  for /f "tokens=*" %%B in ("%%A") do set "CHANNEL=%%~B"
)
if not defined CHANNEL (
  echo [gatedspark] no channel = "..." line in rust-toolchain.toml, so there is no
  echo             pinned version to use. Refusing to guess one.
  exit /b 1
)

set "TOOLCHAIN=%CHANNEL%-x86_64-pc-windows-gnu"

rem Only 64-bit Intel and AMD Windows is wired up. ARM64 Windows is untested here;
rem saying so is better than quietly selecting a toolchain nobody has run.
if /i "%PROCESSOR_ARCHITECTURE%"=="ARM64" (
  echo [gatedspark] note: this is an ARM64 Windows machine. Using %TOOLCHAIN%,
  echo             which runs under emulation and has not been tested.
)

rustup run %TOOLCHAIN% rustc -vV >nul 2>&1
if errorlevel 1 (
  echo [gatedspark] installing %TOOLCHAIN% ^(one time, downloads from rust-lang.org^)
  rustup toolchain install %TOOLCHAIN% --profile minimal --component rustfmt --component clippy
  if errorlevel 1 (
    echo [gatedspark] install failed. Nothing was built.
    exit /b 1
  )
)

cargo +%TOOLCHAIN% %*
exit /b %ERRORLEVEL%
