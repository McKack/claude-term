"""Builds Clauracle for the OS it runs on: Windows dist/Clauracle.exe, macOS dist/Clauracle.app, Linux dist/Clauracle.
Uses the installed `claude` and `gh` at runtime (the SDK's bundled CLI is left out). Cross-OS builds: .github/workflows.
    python build.py
"""
import os
import sys
import time
from pathlib import Path

import PyInstaller.__main__

os.chdir(Path(__file__).parent)
sep = os.pathsep  # PyInstaller's src;dest separator differs per OS
args = ["clauracle.py", "--name", "Clauracle", "--windowed", "--noconfirm",
        f"--add-data=index.html{sep}.", f"--add-data=plugin{sep}plugin", "--exclude-module", "tkinter"]

if sys.platform == "win32":
    # A running exe can't be overwritten but can be renamed: park it so a rebuild works while the app is open.
    exe = Path("dist/Clauracle.exe")
    for old in Path("dist").glob("Clauracle.old-*.exe"):
        try:
            old.unlink()
        except OSError:  # ponytail: one still running stays until the next build
            pass
    if exe.exists():
        exe.rename(f"dist/Clauracle.old-{time.strftime('%Y%m%d%H%M%S')}.exe")
    import winpty  # setup-token runs in a pseudo-console; its helper exes must sit next to its dlls
    wp = Path(winpty.__file__).parent
    args += ["--onefile", "--collect-binaries", "winpty",
             f"--add-binary={wp / 'OpenConsole.exe'}{sep}winpty", f"--add-binary={wp / 'winpty-agent.exe'}{sep}winpty"]
elif sys.platform == "darwin":
    args += ["--osx-bundle-identifier", "no.salnorth.clauracle"]  # --windowed makes the .app
else:
    args += ["--onefile"]

PyInstaller.__main__.run(args)
