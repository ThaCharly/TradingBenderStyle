"""Verifica que los enlaces relativos de los Markdown del repo apunten a archivos existentes."""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")
SKIP_DIRS = {".git", "node_modules", "target", ".venv", "dist"}


def main() -> int:
    broken = []
    for md in ROOT.rglob("*.md"):
        if SKIP_DIRS.intersection(md.parts):
            continue
        for target in LINK.findall(md.read_text(encoding="utf-8")):
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            path = target.split("#", 1)[0]
            if path and not (md.parent / path).exists():
                broken.append(f"{md.relative_to(ROOT)} -> {target}")
    for line in broken:
        print(f"enlace roto: {line}")
    return 1 if broken else 0


if __name__ == "__main__":
    sys.exit(main())
