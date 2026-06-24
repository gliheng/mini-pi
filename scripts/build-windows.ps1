param(
    [string]$Version = "0.1.0"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$root = Split-Path -Parent $PSScriptRoot
$package = Join-Path $root "target\package\mini-pi"
$tools = Join-Path $root "tools"
$wixOut = Join-Path $root "target\wix"
$msiOut = Join-Path $root "target\mini-pi-$Version-x64.msi"

New-Item -ItemType Directory -Force -Path $tools | Out-Null
New-Item -ItemType Directory -Force -Path $wixOut | Out-Null

Write-Host "Building release binary..."
cargo build --release

Write-Host "Staging installer files..."
if (Test-Path $package) {
    Remove-Item -Recurse -Force $package
}
New-Item -ItemType Directory -Force -Path $package | Out-Null

Copy-Item (Join-Path $root "target\release\mini-pi.exe") $package
Copy-Item -Recurse (Join-Path $root "assets") $package

$bridgeStage = Join-Path $package "pi-bridge"
New-Item -ItemType Directory -Force -Path $bridgeStage | Out-Null
Copy-Item (Join-Path $root "pi-bridge\package.json") $bridgeStage
Copy-Item (Join-Path $root "pi-bridge\package-lock.json") $bridgeStage
Copy-Item -Recurse (Join-Path $root "pi-bridge\dist") $bridgeStage

Write-Host "Installing production Node dependencies for pi-bridge..."
Push-Location $bridgeStage
& npm ci --omit=dev
if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
Pop-Location

$nodeVersion = "20.15.1"
$nodeZip = Join-Path $tools "node-v$nodeVersion-win-x64.zip"
$nodeExtract = Join-Path $tools "node-v$nodeVersion-win-x64"
if (-not (Test-Path $nodeZip)) {
    Write-Host "Downloading Node.js v$nodeVersion ..."
    Invoke-WebRequest -Uri "https://nodejs.org/dist/v$nodeVersion/node-v$nodeVersion-win-x64.zip" -OutFile $nodeZip
}
if (-not (Test-Path $nodeExtract)) {
    Expand-Archive -Path $nodeZip -DestinationPath $tools -Force
}
Copy-Item (Join-Path $nodeExtract "node.exe") $package

$wixZip = Join-Path $tools "wix311-binaries.zip"
$wixDir = Join-Path $tools "wix"
if (-not (Test-Path $wixZip)) {
    Write-Host "Downloading WiX v3.11.2 binaries ..."
    Invoke-WebRequest -Uri "https://github.com/wixtoolset/wix3/releases/download/wix3112rtm/wix311-binaries.zip" -OutFile $wixZip
}
if (-not (Test-Path $wixDir)) {
    Expand-Archive -Path $wixZip -DestinationPath $wixDir -Force
}
$env:PATH = "$wixDir;$env:PATH"

$filesWxs = Join-Path $wixOut "files.wxs"
Write-Host "Harvesting staged files with heat..."
& heat dir "$package" -cg ApplicationFiles -gg -sfrag -srd -dr INSTALLFOLDER -out "$filesWxs"
if ($LASTEXITCODE -ne 0) { throw "heat failed" }

Write-Host "Compiling MSI..."
& candle -arch x64 -out "$wixOut\" (Join-Path $root "wix\main.wxs") "$filesWxs"
if ($LASTEXITCODE -ne 0) { throw "candle failed" }

& light -out "$msiOut" "$wixOut\main.wixobj" "$wixOut\files.wixobj"
if ($LASTEXITCODE -ne 0) { throw "light failed" }

Write-Host "Installer created: $msiOut"
