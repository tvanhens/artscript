"""Fail when the language card or a help page is over its token budget in bench/budget.toml.

    uv run --with tiktoken python scripts/card_budget.py
"""

import sys
import tomllib

from corpus import ROOT, count

BUDGET = ROOT / "bench" / "budget.toml"
RATCHET_STEP = 50


def main() -> None:
    budget = tomllib.loads(BUDGET.read_text())
    card, help_pages = budget["card"], budget["help"]
    over = []

    path = ROOT / card["path"]
    tokens = count(path.read_text())
    print(f"{card['path']}: {tokens} / {card['tokens']} tokens")
    if tokens > card["tokens"]:
        over.append(card["path"])
    elif card["tokens"] - tokens >= RATCHET_STEP:
        lower = -(-tokens // RATCHET_STEP) * RATCHET_STEP
        print(f"  the card shrank: lower [card] tokens in {BUDGET.relative_to(ROOT)} to {lower}")

    pages = sorted(ROOT.glob(help_pages["glob"]))
    for page in pages:
        tokens = count(page.read_text())
        print(f"{page.relative_to(ROOT)}: {tokens} / {help_pages['tokens']} tokens")
        if tokens > help_pages["tokens"]:
            over.append(str(page.relative_to(ROOT)))
    if not pages:
        print(f"no help pages match {help_pages['glob']}")

    if over:
        sys.exit(f"over budget: {', '.join(over)}")


if __name__ == "__main__":
    main()
