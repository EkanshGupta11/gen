$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path (Get-Location).Path 'work\cargo-target' }
cargo test --locked --offline
$venvPython = if (Test-Path '.venv\Scripts\python.exe') { '.venv\Scripts\python.exe' } elseif (Test-Path '.venv\bin\python.exe') { '.venv\bin\python.exe' } else { 'python' }
& $venvPython -m unittest discover -s engine -p 'test_*.py' -v
