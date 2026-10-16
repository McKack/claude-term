# Builds dist\claude-term.exe (uses your installed `claude`; the SDK's bundled CLI is left out).
# -Install adds this repo's dist\ folder to your user PATH (no copy, so every rebuild is live).
param([switch]$Install)
Set-Location $PSScriptRoot

# A running exe can't be overwritten but can be renamed: park it so we can rebuild from inside claude-term.
# Running sessions keep the old copy; new launches get the fresh build.
$exe = "dist\claude-term.exe"
Remove-Item dist\claude-term.old*.exe -ErrorAction SilentlyContinue   # ponytail: fails silently while an old one still runs
if (Test-Path $exe) { Move-Item $exe "dist\claude-term.old-$(Get-Date -Format yyyyMMddHHmmss).exe" }

python -m PyInstaller --onefile --name claude-term --noconfirm `
    --collect-submodules textual --collect-data textual --collect-submodules rich `
    --exclude-module tkinter --exclude-module PIL claude_term.py
if ($LASTEXITCODE) { exit $LASTEXITCODE }

if ($Install) {
    $dir = Join-Path $PSScriptRoot "dist"
    $path = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($path -split ";") -notcontains $dir) {
        [Environment]::SetEnvironmentVariable("Path", "$($path.TrimEnd(';'));$dir", "User")
        "Added $dir to your PATH. Open a new terminal and run: claude-term"
    }
}
