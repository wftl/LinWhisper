param(
  [ValidateSet("dev", "prod")]
  [string]$Mode = "dev"
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

switch ($Mode) {
  "dev" {
    npm run tauri dev
    exit $LASTEXITCODE
  }
  "prod" {
    & "$PSScriptRoot/build.ps1" release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & "$PSScriptRoot/src-tauri/target/release/whispertray.exe"
    exit $LASTEXITCODE
  }
}
