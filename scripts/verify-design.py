#!/usr/bin/env python3
"""Tools' UI uses the Arcade design kit and its wording (Link design/STANDARD.md §3-4, §9, §11).

    python3 scripts/verify-design.py

Checks everything outside the synced kit (src-ui/design/):
- no colour literals and no px font sizes in Tools' own styles, and fonts only through the kit's variables;
- every var(--token) used is defined by the kit or by Tools;
- padding, margin and gap in px stay on the kit's spacing scale (hairline 1 px allowed);
- user-facing messages read "Couldn't ...", never "Could not", "Failed to" or "Unable to" (UI and Rust).
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
UI = ROOT / "src-ui"
KIT = UI / "design"
SCALE = {0, 1, 2, 4, 6, 8, 10, 12, 14, 16, 20, 24, 32, 40, 48}
COLOUR = re.compile(r"#[0-9a-fA-F]{3,8}\b|\b(?:rgb|rgba|hsl|hsla)\(")
FONT_SIZE = re.compile(r"font-size\s*:\s*[^;}\"]*\d+(?:\.\d+)?px")
FONT_FAMILY = re.compile(r"font-family\s*:\s*([^;}\"]+)")
SPACING = re.compile(r"\b(?:padding|margin|gap|row-gap|column-gap)(?:-[a-z]+)?\s*:\s*([^;}\"]+)")
PX = re.compile(r"-?(\d+(?:\.\d+)?)px")
DEFINED = re.compile(r"(?:^|[;{\s])(--[a-z0-9-]+)\s*:")
USED = re.compile(r"var\((--[a-z0-9-]+)")
WORDING = re.compile(r"[\"'`](?:Could not|Failed to|Unable to)\b")
LOGGING = re.compile(r"\b(?:eprintln|println|log::\w+|tracing::\w+|warn|info|debug|trace|error)!\(")


def style_blocks(path, text):
    """The CSS of a .css file, or the <style> blocks of a .svelte file (markup holds no styles of its own)."""
    if path.suffix == ".css":
        return text
    return "\n".join(re.findall(r"<style[^>]*>(.*?)</style>", text, re.S))


def line_of(text, index):
    return text.count("\n", 0, index) + 1


def main():
    problems = []
    defined = set()
    for path in UI.rglob("*"):
        if path.suffix in (".css", ".svelte", ".ts"):
            defined.update(DEFINED.findall(path.read_text(encoding="utf-8")))
    own = [p for p in sorted(UI.rglob("*")) if p.suffix in (".css", ".svelte", ".ts") and KIT not in p.parents]
    for path in own:
        text = path.read_text(encoding="utf-8")
        rel = path.relative_to(ROOT)
        for token in USED.findall(text):
            if token not in defined:
                problems.append(f"{rel}: var({token}) is not defined by the kit or Tools")
        if path.suffix == ".ts":
            continue
        css = style_blocks(path, text)
        for match in COLOUR.finditer(css):
            problems.append(f"{rel}: colour literal {match.group(0)!r}; use a kit colour token")
        for match in FONT_SIZE.finditer(css):
            problems.append(f"{rel}: px font size {match.group(0)!r}; use a kit type token")
        for match in FONT_FAMILY.finditer(css):
            family = match.group(1).strip()
            if family not in ("inherit", "Inter") and not family.startswith("var(--font-"):
                problems.append(f"{rel}: font-family {family!r}; use var(--font-sans) or var(--font-mono)")
        for match in SPACING.finditer(css):
            for value in PX.findall(match.group(1)):
                if float(value) not in SCALE:
                    problems.append(f"{rel}: {match.group(0).strip()!r} is off the kit's spacing scale")
    sources = own + sorted((ROOT / "src").rglob("*.rs")) + sorted((ROOT / "src-tauri/src").rglob("*.rs"))
    for path in sources:
        text = path.read_text(encoding="utf-8")
        for match in WORDING.finditer(text):
            line = text.splitlines()[line_of(text, match.start()) - 1]
            if not LOGGING.search(line):
                problems.append(f"{path.relative_to(ROOT)}:{line_of(text, match.start())}: say \"Couldn't ...\" "
                                f"(STANDARD §9 error wording): {line.strip()}")
    if problems:
        print("\n".join(problems))
        print(f"{len(problems)} design kit problems")
        return 1
    print(f"Design kit: {len(own)} UI files use kit colours, type, tokens and spacing; messages use kit wording")
    return 0


if __name__ == "__main__":
    sys.exit(main())
