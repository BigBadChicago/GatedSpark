<#
    One-time setup for building GatedSpark on Windows. Safe to run more than once.

    Why this exists: a "linker" is the last step of a build, the part that joins the
    compiled pieces into one program you can run. Rust's default Windows setup borrows
    that linker from Visual Studio, so on a machine without Visual Studio the build
    stops with "linker `link.exe` not found" before any test runs.

    This switches Rust's default host to the "gnu" flavour, which brings its own
    linker, and installs the version pinned in rust-toolchain.toml. The version is
    read from that file rather than written here, so this script cannot drift from the
    pin.

    See docs/adr/0005-windows-gnu-toolchain.md for the trade-offs, including why the
    host is deliberately not pinned in rust-toolchain.toml.
#>

$ErrorActionPreference = 'Stop'

$triple = 'x86_64-pc-windows-gnu'
$repo = Split-Path -Parent $PSScriptRoot
$pin = Join-Path $repo 'rust-toolchain.toml'

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    throw 'rustup was not found on PATH. Install Rust from https://rustup.rs and run this again.'
}
if (-not (Test-Path $pin)) {
    throw "Could not find $pin. Run this script from inside the repository."
}

$match = Select-String -Path $pin -Pattern '^\s*channel\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $match) {
    throw "Could not read the pinned channel from $pin."
}
$channel = $match.Matches[0].Groups[1].Value
$toolchain = "$channel-$triple"

Write-Host "Pinned channel:  $channel"
Write-Host "Installing:      $toolchain"
Write-Host ''

rustup toolchain install $toolchain
if ($LASTEXITCODE -ne 0) { throw "rustup toolchain install $toolchain failed." }

Write-Host ''
Write-Host "Setting this machine's default Rust host to $triple ..."
rustup set default-host $triple
if ($LASTEXITCODE -ne 0) { throw "rustup set default-host $triple failed." }

Write-Host ''
Write-Host 'Checking that it worked:'
Push-Location $repo
try {
    rustup show active-toolchain
    cargo --version
}
finally {
    Pop-Location
}

Write-Host ''
Write-Host 'Done. Now run:  cargo test --workspace'
