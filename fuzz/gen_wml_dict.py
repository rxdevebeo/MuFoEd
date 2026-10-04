"""Build libFuzzer dictionary entries from coverage/wml-elements.toml (AUD-92)."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOML = ROOT / "coverage" / "wml-elements.toml"
OUT = Path(__file__).resolve().parent / "dict" / "wml.dict"


def main() -> None:
    text = TOML.read_text(encoding="utf-8")
    names = re.findall(r'name\s*=\s*"([^"]+)"', text)
    lines: list[str] = []
    for name in names:
        lines.append(f'"{name}"')
        if ":" in name:
            local = name.split(":", 1)[1]
            lines.append(f'"{local}"')
            lines.append(f'"<{name}"')
            lines.append(f'"</{name}>"')
    unique = list(dict.fromkeys(lines))
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(unique) + "\n", encoding="utf-8")
    print(f"wrote {OUT} ({len(unique)} entries)")


if __name__ == "__main__":
    main()
