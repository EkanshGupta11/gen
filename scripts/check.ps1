$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path (Get-Location).Path 'work\cargo-target' }
$components = rustup component list --installed
if ($components -match '^rustfmt-') { cargo fmt --check } else { Write-Warning 'rustfmt is not installed; skipping formatting check.' }
if ($components -match '^clippy-') { cargo clippy --locked --all-targets -- -D warnings } else { Write-Warning 'Clippy is not installed; skipping lint check.' }
cargo check --locked --offline
$venvPython = if (Test-Path '.venv\Scripts\python.exe') { '.venv\Scripts\python.exe' } elseif (Test-Path '.venv\bin\python.exe') { '.venv\bin\python.exe' } else { 'python' }
& $venvPython -m py_compile engine/app.py engine/test_app.py
Write-Host 'Checks complete.'
