# Installs Launchpad into this checkout's bin\ for the herdr plugin: the
# release archive for this plugin version on x64, otherwise a build with Cargo.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$match = Select-String -Path 'herdr-plugin.toml' -Pattern '^version = "(.*)"$' | Select-Object -First 1
if (-not $match) { throw 'No version in herdr-plugin.toml' }
$version = $match.Matches[0].Groups[1].Value
$release = "https://github.com/JimiSmith/launchpad/releases/download/v$version"
$bin = Join-Path $root 'bin'
New-Item -ItemType Directory -Force -Path $bin | Out-Null

if ($env:PROCESSOR_ARCHITECTURE -eq 'AMD64') {
    # A download or checksum failure stops the install.
    $archive = 'launchpad-windows-x86_64.zip'
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        Write-Output "Downloading $archive for v$version"
        Invoke-WebRequest -UseBasicParsing -Uri "$release/$archive" -OutFile "$tmp\$archive"
        Invoke-WebRequest -UseBasicParsing -Uri "$release/$archive.sha256" -OutFile "$tmp\$archive.sha256"
        $expected = ((Get-Content "$tmp\$archive.sha256" -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
        $actual = (Get-FileHash "$tmp\$archive" -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($expected -ne $actual) {
            throw "Checksum mismatch for $archive (expected $expected, got $actual)"
        }
        Expand-Archive -Path "$tmp\$archive" -DestinationPath "$tmp\x"
        Copy-Item -Force "$tmp\x\launchpad.exe" (Join-Path $bin 'launchpad.exe')
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
} else {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "No release archive for $env:PROCESSOR_ARCHITECTURE Windows, and cargo is not installed. Install Rust from https://rustup.rs, then reinstall the plugin."
    }
    Write-Output 'Building Launchpad with Cargo; this takes a few minutes'
    # Build outside the checkout, which herdr keeps for as long as the plugin.
    $target = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
    $env:CARGO_TARGET_DIR = $target
    try {
        & cargo build --release --locked -p launchpad
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
        Copy-Item -Force "$target\release\launchpad.exe" (Join-Path $bin 'launchpad.exe')
    } finally {
        Remove-Item -Recurse -Force $target -ErrorAction SilentlyContinue
    }
}
$installed = & (Join-Path $bin 'launchpad.exe') --version
Write-Output "Installed $installed"
