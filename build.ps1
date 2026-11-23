param(
  [ValidateSet("debug", "release")]
  [string]$Mode = "debug"
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

if ($Mode -eq "release") {
  Write-Host "Building release version..."
  npm run tauri build -- --no-bundle
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Write-Host "Binary: src-tauri/target/release/whispertray.exe"
} else {
  Write-Host "Building debug version..."
  cargo build --manifest-path src-tauri/Cargo.toml
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Write-Host "Binary: src-tauri/target/debug/whispertray.exe"
}
