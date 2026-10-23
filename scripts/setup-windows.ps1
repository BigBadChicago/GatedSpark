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
Write-Host 'Checking that the GNU build tools are complete ...'

# Why this check exists: switching the host and installing the toolchain is not enough on
# its own. The gnu flavour reaches Windows system libraries through a helper called
# dlltool, which arrives in rustup's `rust-mingw` piece. When it is missing the build
# fails deep inside a dependency with "error calling dlltool 'dlltool.exe': program not
# found", which says nothing about what to install. A setup script that reports a machine
# is ready when it is not is worse than no setup script, so this asks the question here
# where the answer can be explained.
$dlltool = Get-Command dlltool.exe -ErrorAction SilentlyContinue
if (-not $dlltool) {
    $root = (rustup show home).Trim()
    $bundled = Join-Path $root "toolchains\$toolchain\lib\rustlib\$triple\bin\self-contained\dlltool.exe"
    if (Test-Path $bundled) {
        $dlltool = $bundled
    }
}

if ($dlltool) {
    Write-Host "  dlltool found: $dlltool"
} else {
    Write-Host ''
    Write-Warning @'
dlltool.exe was not found.

Without it the build stops inside the `getrandom` crate with:
    error: error calling dlltool 'dlltool.exe': program not found

dlltool is part of MinGW, which the GNU toolchain is built on. Two ways to get it:

  1. Reinstall the piece it ships in:
       rustup component add rust-mingw --toolchain TOOLCHAIN

  2. Or install MinGW-w64 and put its bin folder on PATH:
       winget install -e --id MSYS2.MSYS2
     then add C:\msys64\mingw64\bin to PATH and open a new terminal.

Everything else above succeeded. Run this script again after either one.
'@.Replace('TOOLCHAIN', $toolchain)
    exit 1
}

Write-Host ''
Write-Host 'Done. Now run:  cargo test --workspace'
