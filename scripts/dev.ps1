$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path (Get-Location).Path 'work\cargo-target' }
if (Test-Path '.env') {
    Get-Content '.env' | ForEach-Object {
        if ($_ -match '^\s*([^#=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1].Trim(), $matches[2].Trim(), 'Process') }
    }
}
# Prefer CPython 3.13 venv, then legacy .venv, then .env setting
$venvPython = if (Test-Path '.venv-cpython313\Scripts\python.exe') { (Resolve-Path '.venv-cpython313\Scripts\python.exe').Path } elseif (Test-Path '.venv\Scripts\python.exe') { (Resolve-Path '.venv\Scripts\python.exe').Path } elseif (Test-Path '.venv\bin\python.exe') { (Resolve-Path '.venv\bin\python.exe').Path } else { $null }
if ($venvPython -and ($env:PYTHON_EXECUTABLE -eq 'python' -or -not $env:PYTHON_EXECUTABLE -or $env:PYTHON_EXECUTABLE -like '*.venv*')) { $env:PYTHON_EXECUTABLE = $venvPython }
cargo run
