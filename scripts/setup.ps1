$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path (Get-Location).Path 'work\cargo-target' }
Write-Host 'Checking local prerequisites...'
rustc --version
cargo --version

# Prefer uv-managed CPython 3.13 for scientific package compatibility.
# The MSYS2/UCRT Python 3.14 environment cannot install NumPy/SciPy/SymPy wheels.
$uvAvailable = Get-Command uv -ErrorAction SilentlyContinue
if (-not $uvAvailable) {
    Write-Error 'Astral uv is required. Install from https://docs.astral.sh/uv/getting-started/installation/'
    exit 1
}
Write-Host "uv $(uv --version)"

# Install CPython 3.13 if not present
uv python install 3.13

# Create a project-local virtual environment with standard CPython 3.13
if (-not (Test-Path '.venv-cpython313')) {
    uv venv .venv-cpython313 --python 3.13
}
$venvPython = '.venv-cpython313\Scripts\python.exe'
& $venvPython --version

# Install scientific dependencies
uv pip install --python $venvPython -r requirements.txt
Write-Host 'Scientific packages installed.'

# Verify imports
& $venvPython -c "import numpy, scipy, sympy, pandas, matplotlib; print('Scientific packages verified.')"

if (-not (Test-Path '.env')) { Copy-Item '.env.example' '.env' }
cargo build --locked
Write-Host 'Setup complete.'
