"""Attachments: native file picker, clipboard images/files, and turning files into message content."""
import asyncio
import base64
import os
import time
from pathlib import Path

PASTES = Path.home() / ".claude-term" / "pastes"
IMAGE_TYPES = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif",
               ".webp": "image/webp"}
MAX_IMAGE, MAX_PDF, MAX_TEXT = 5_000_000, 30_000_000, 200_000  # API limits / sane inline size


async def _powershell(script):
    proc = await asyncio.create_subprocess_exec(
        "powershell", "-NoProfile", "-STA", "-Command", script,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL)
    out, _ = await proc.communicate()
    return [line.strip() for line in out.decode(errors="replace").splitlines() if line.strip()]


async def pick_files():
    """The Windows file dialog, kept on top of the terminal. [] when cancelled."""
    lines = await _powershell(
        "Add-Type -AssemblyName System.Windows.Forms;"
        "$f = New-Object Windows.Forms.Form -Property @{TopMost=$true};"
        "$d = New-Object Windows.Forms.OpenFileDialog -Property @{Multiselect=$true; Title='Attach files'};"
        "if ($d.ShowDialog($f) -eq 'OK') { $d.FileNames }")
    return [p for p in lines if os.path.isfile(p)]


async def clipboard_files():
    """Files copied in Explorer, or a clipboard image saved as PNG. [] when it holds neither."""
    PASTES.mkdir(parents=True, exist_ok=True)
    target = PASTES / time.strftime("paste-%Y%m%d-%H%M%S.png")
    lines = await _powershell(
        "Add-Type -AssemblyName System.Windows.Forms, System.Drawing;"
        "if ([Windows.Forms.Clipboard]::ContainsFileDropList()) { [Windows.Forms.Clipboard]::GetFileDropList() }"
        "elseif ([Windows.Forms.Clipboard]::ContainsImage()) {"
        f" [Windows.Forms.Clipboard]::GetImage().Save('{target}', [System.Drawing.Imaging.ImageFormat]::Png); '{target}' }}")
    return [p for p in lines if os.path.isfile(p)]


def paths_in(text):
    """Pasted text that is nothing but existing file paths (e.g. dragged in) -> those paths."""
    parts = [p.strip().strip('"') for p in text.strip().splitlines() if p.strip()]
    return parts if parts and all(os.path.isfile(p) for p in parts) else []


def content_blocks(text, paths):
    """User message content: images and PDFs inline, small text files inline, anything else by path."""
    blocks = []
    for p in paths:
        ext, size = Path(p).suffix.lower(), os.path.getsize(p)
        if ext in IMAGE_TYPES and size <= MAX_IMAGE:
            blocks.append({"type": "image", "source": {"type": "base64", "media_type": IMAGE_TYPES[ext],
                                                       "data": base64.b64encode(Path(p).read_bytes()).decode()}})
            continue
        if ext == ".pdf" and size <= MAX_PDF:
            blocks.append({"type": "document", "source": {"type": "base64", "media_type": "application/pdf",
                                                          "data": base64.b64encode(Path(p).read_bytes()).decode()}})
            continue
        if size <= MAX_TEXT:
            try:
                blocks.append({"type": "text", "text": f'<file path="{p}">\n{Path(p).read_text(encoding="utf-8")}\n</file>'})
                continue
            except (UnicodeDecodeError, OSError):
                pass
        blocks.append({"type": "text", "text": f"Attached file (too big or binary; open it with your tools if needed): {p}"})
    blocks.append({"type": "text", "text": text or "See the attached files."})
    return blocks


if __name__ == "__main__":
    import tempfile
    d = Path(tempfile.mkdtemp())
    (d / "a.png").write_bytes(b"\x89PNG fake")
    (d / "b.txt").write_text("hello", encoding="utf-8")
    (d / "c.bin").write_bytes(b"\xff\xfe\x00bad")
    kinds = [b["type"] for b in content_blocks("hi", [str(d / "a.png"), str(d / "b.txt"), str(d / "c.bin")])]
    assert kinds == ["image", "text", "text", "text"], kinds
    assert paths_in(f'"{d / "b.txt"}"\n') == [str(d / "b.txt")] and paths_in("just words") == []
    print("ok")
