$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path (Get-Location).Path 'work\cargo-target' }

# Python executable resolution
$venvPython = if (Test-Path '.venv-cpython313\Scripts\python.exe') { (Resolve-Path '.venv-cpython313\Scripts\python.exe').Path } elseif (Test-Path '.venv\Scripts\python.exe') { (Resolve-Path '.venv\Scripts\python.exe').Path } elseif (Test-Path '.venv\bin\python.exe') { (Resolve-Path '.venv\bin\python.exe').Path } else { 'python' }
$env:PYTHON_EXECUTABLE = $venvPython

Write-Host "=== 1. Rust Test Suite ==="
cargo test --locked --offline

Write-Host "`n=== 2. Python Test Suite ==="
& $venvPython -m unittest discover -s engine -p 'test_*.py' -v

Write-Host "`n=== 3. Frontend Syntax Check ==="
& $venvPython -c "
import re, subprocess, sys
html = open('web/index.html', encoding='utf-8').read()
match = re.search(r'<script>(.*?)</script>', html, re.DOTALL)
if match:
    code = match.group(1)
    p = subprocess.run(['node', '--check'], input=code, text=True, capture_output=True)
    if p.returncode == 0:
        print('Frontend JS syntax check: PASSED')
    else:
        print('Frontend JS syntax check FAILED:', p.stderr)
        sys.exit(1)
"

Write-Host "`n=== 4. Cross-Process Persistence Recovery Test ==="
& $venvPython scripts/test_persistence_recovery.py
