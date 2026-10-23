<#
    Upload a published folder to an Amazon S3 bucket, so that somebody who is not you
    can play. The PowerShell twin of scripts/deploy-s3.sh, for a Windows machine with
    no bash.

    `gatedspark publish` writes the folder. This puts it on a web host. Nothing here
    decides anything about a puzzle: every check still happens in the browser that
    opens it, which is why an ordinary bucket of files is enough and there is no
    program running on the server.

    Usage:
        scripts\deploy-s3.ps1 .\site jbravo-gated-spark us-east-2

    What the bucket needs first is in docs/deploying-to-s3.md. Run that once; this
    script is the part you run every time.

    It needs the AWS command line tool on PATH and an AWS login already set up
    (`aws configure`). It does not read or write any key of its own.
#>

param(
    [Parameter(Mandatory = $true)][string]$Site,
    [Parameter(Mandatory = $true)][string]$Bucket,
    [string]$Region = 'us-east-2'
)

$ErrorActionPreference = 'Stop'

if (-not (Get-Command aws -ErrorAction SilentlyContinue)) {
    throw 'The AWS command line tool was not found on PATH. Install it from https://aws.amazon.com/cli/ and run `aws configure` first.'
}
if (-not (Test-Path (Join-Path $Site 'index.html'))) {
    throw "There is no page in $Site. It should be a folder written by: gatedspark publish --out <dir> --page crates/gs-web/www"
}
if (-not (Test-Path (Join-Path $Site 'store/schedule.gspk'))) {
    throw "There is no schedule in $Site, so a visitor would have nothing to open."
}

# Publishing the answers is the one mistake that cannot be taken back: once they are on
# a public host they have been seen. `gatedspark publish` never copies them, and this is
# the second lock on the same door, because this script is pointed at a folder somebody
# chose and nothing stops that folder being a store.
$answers = Get-ChildItem -Path $Site -Filter 'witness.bin' -Recurse -File -ErrorAction SilentlyContinue
if ($answers) {
    Write-Error @"
REFUSING: $Site contains answer files (witness.bin).
That looks like a store rather than a published folder. Uploading it would
publish the answer to every puzzle. Use 'gatedspark publish --out' instead.
"@
    exit 1
}

function Invoke-Aws {
    param([string[]]$Arguments)
    & aws @Arguments
    if ($LASTEXITCODE -ne 0) { throw "aws $($Arguments -join ' ') failed with exit code $LASTEXITCODE." }
}

Write-Host "==> uploading $Site to s3://$Bucket ($Region)"

# Three kinds of file, three sets of rules.
#
# **The puzzles never change.** A puzzle's name is its own fingerprint, so the file at
# that name can never be a different file: if the bytes changed, the name would change.
# That is what lets them be cached forever, and it is a property of how they are named
# rather than a promise anybody has to keep.
#
# The browser module and the page's own code are the same shape of thing per build, but
# their names do NOT carry a fingerprint, so they get an ordinary short cache.
#
# **The schedule and the withdrawal list must not be cached for long.** Both are the
# files that say what has changed. A cached withdrawal list is a withdrawal that has not
# arrived yet, and that is the one delay in this whole design that actually hurts
# somebody: a player carries on with a puzzle that cannot be finished. The signature and
# the serial number stop an old file being *forced* on a device; only a short cache stops
# it arriving late.

# The puzzles: immutable, and named by their own fingerprint.
Invoke-Aws @(
    's3', 'sync', (Join-Path $Site 'store'), "s3://$Bucket/store",
    '--region', $Region,
    '--exclude', 'schedule.gspk', '--exclude', 'withdrawals.gspk',
    '--content-type', 'application/octet-stream',
    '--cache-control', 'public, max-age=31536000, immutable',
    '--no-progress'
)

# The two files that say what has changed. Uploaded after the puzzles, so a schedule
# never names a day whose puzzle is not up yet.
Invoke-Aws @(
    's3', 'cp', (Join-Path $Site 'store/schedule.gspk'), "s3://$Bucket/store/schedule.gspk",
    '--region', $Region,
    '--content-type', 'application/octet-stream',
    '--cache-control', 'no-cache',
    '--no-progress'
)
if (Test-Path (Join-Path $Site 'store/withdrawals.gspk')) {
    Invoke-Aws @(
        's3', 'cp', (Join-Path $Site 'store/withdrawals.gspk'), "s3://$Bucket/store/withdrawals.gspk",
        '--region', $Region,
        '--content-type', 'application/octet-stream',
        '--cache-control', 'no-cache',
        '--no-progress'
    )
}

# The page. `.wasm` is the one that has to be right rather than merely tidy: a browser
# refuses to start a WebAssembly module that does not arrive as `application/wasm`, and
# the failure reads as a broken page rather than a wrong file type.
Invoke-Aws @(
    's3', 'sync', $Site, "s3://$Bucket",
    '--region', $Region,
    '--exclude', 'store/*', '--exclude', '*.wasm', '--exclude', '*.js', '--exclude', '*.html',
    '--cache-control', 'public, max-age=300',
    '--no-progress'
)
Invoke-Aws @(
    's3', 'sync', $Site, "s3://$Bucket",
    '--region', $Region,
    '--exclude', '*', '--include', '*.wasm',
    '--content-type', 'application/wasm',
    '--cache-control', 'public, max-age=300',
    '--no-progress'
)
Invoke-Aws @(
    's3', 'sync', $Site, "s3://$Bucket",
    '--region', $Region,
    '--exclude', '*', '--include', '*.js',
    '--content-type', 'text/javascript; charset=utf-8',
    '--cache-control', 'public, max-age=300',
    '--no-progress'
)
Invoke-Aws @(
    's3', 'sync', $Site, "s3://$Bucket",
    '--region', $Region,
    '--exclude', '*', '--include', '*.html',
    '--content-type', 'text/html; charset=utf-8',
    '--cache-control', 'no-cache',
    '--no-progress'
)

Write-Host ''
Write-Host '==> done. The page is at:'
Write-Host "    http://$Bucket.s3-website.$Region.amazonaws.com/"
Write-Host ''
Write-Host 'If that address says 404, static website hosting is not turned on for the bucket'
Write-Host 'yet. See docs/deploying-to-s3.md.'
