$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path (Get-Location).Path 'work\cargo-target' }
Write-Host 'Checking local prerequisites...'
rustc --version
cargo --version
python --version
if (-not (Test-Path '.venv')) { python -m venv .venv }
$venvPython = if (Test-Path '.venv\Scripts\python.exe') { '.venv\Scripts\python.exe' } else { '.venv\bin\python.exe' }
& $venvPython --version
& $venvPython -m pip --version
Write-Host 'Rust dependencies are resolved from the lockfile. Python scientific libraries are not yet required by this MVP.'
if (-not (Test-Path '.env')) { Copy-Item '.env.example' '.env' }
cargo build --locked
