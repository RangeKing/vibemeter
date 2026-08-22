#!/usr/bin/env python3
"""Fetch official model prices and regenerate VibeMeter's pricing catalog.

The parser deliberately fails closed: a provider page that changes shape or
loses its pricing table stops a release instead of silently keeping stale
prices. ``--fixture-dir`` is provided for deterministic offline tests.
"""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import re
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from html.parser import HTMLParser
from pathlib import Path
from typing import Iterable
from urllib.parse import urljoin
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "apps/desktop/src-tauri/pricing.generated.json"
USER_AGENT = "VibeMeter-pricing-updater/1.0 (+https://github.com/RangeKing/vibemeter)"


SOURCES = {
    "openai": "https://developers.openai.com/api/docs/pricing",
    "anthropic": "https://platform.claude.com/docs/en/about-claude/pricing",
    "deepseek": "https://api-docs.deepseek.com/quick_start/pricing/",
    "kimi": "https://platform.kimi.com/docs/pricing/chat",
    "zai": "https://docs.z.ai/guides/overview/pricing",
    "xai": "https://docs.x.ai/developers/models",
    "cursor": "https://cursor.com/docs/models-and-pricing",
}

KIMI_PAGES = {
    "kimi-k3": "https://platform.kimi.com/docs/pricing/chat-k3",
    "kimi-k2.7-code": "https://platform.kimi.com/docs/pricing/chat-k27-code",
    "kimi-k2.7-code-highspeed": "https://platform.kimi.com/docs/pricing/chat-k27-code",
    "kimi-k2.6": "https://platform.kimi.com/docs/pricing/chat-k26",
    "kimi-k2.5": "https://platform.kimi.com/docs/pricing/chat-k25",
}


@dataclass(frozen=True)
class Price:
    name: str
    input: float
    cache_read: float
    output: float
    currency: str = "USD"
    cache_write: float | None = None
    cache_write_1h: float | None = None
    aliases: tuple[str, ...] = ()

    def as_json(self) -> dict[str, object]:
        return {
            "name": self.name,
            "aliases": list(self.aliases),
            "currency": self.currency,
            "input": self.input,
            "cacheRead": self.cache_read,
            "cacheWrite": self.cache_write,
            "cacheWrite1h": self.cache_write_1h,
            "output": self.output,
        }


class TableParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.rows: list[list[str]] = []
        self._row: list[str] | None = None
        self._cell: list[str] | None = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag == "tr":
            self._row = []
        elif tag in {"td", "th"} and self._row is not None:
            self._cell = []

    def handle_data(self, data: str) -> None:
        if self._cell is not None:
            self._cell.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag in {"td", "th"} and self._cell is not None and self._row is not None:
            self._row.append(" ".join("".join(self._cell).split()))
            self._cell = None
        elif tag == "tr" and self._row is not None:
            self.rows.append(self._row)
            self._row = None


def fetch(url: str, fixture: Path | None = None) -> str:
    if fixture is not None:
        return fixture.read_text(encoding="utf-8")
    request = Request(url, headers={"User-Agent": USER_AGENT, "Accept": "text/html,application/json"})
    with urlopen(request, timeout=60) as response:
        return response.read().decode("utf-8", "ignore")


def number(value: str) -> float | None:
    value = html.unescape(value).strip().replace(",", "")
    if value in {"", "-", "—", "null", "None", "N/A"}:
        return None
    if value.lower() in {"free", "limited-time free", "免费", "限时免费"}:
        return 0.0
    match = re.search(r"([0-9]+(?:\.[0-9]+)?)", value)
    return float(match.group(1)) if match else None


def money(value: str) -> float | None:
    return number(value.replace("/ MTok", "").replace("/ 1M tokens", ""))


def normalized(value: str) -> str:
    value = value.strip().lower()
    value = re.sub(r"\s*\(<[^>]+>\)", "", value)
    value = value.replace("claude ", "claude-")
    value = value.replace("grok ", "grok-")
    value = value.replace("glm ", "glm-")
    value = value.replace("composer ", "composer-")
    value = re.sub(r"[^a-z0-9]+", "-", value).strip("-")
    return value


def rows_from_html(source: str) -> list[list[str]]:
    parser = TableParser()
    parser.feed(html.unescape(source))
    return parser.rows


def parse_openai(source: str) -> list[Price]:
    prices: list[Price] = []
    for row in rows_from_html(source):
        if not row:
            continue
        model = normalized(row[0])
        if not model.startswith(("gpt-", "o1", "o3", "o4")) or len(row) < 5:
            continue
        values = [money(cell) for cell in row[1:5]]
        if any(value is None for value in values):
            continue
        prices.append(Price(model, values[0], values[1], values[3], cache_write=values[2]))
    return unique_prices(prices)


def parse_anthropic(source: str) -> list[Price]:
    prices: list[Price] = []
    for row in rows_from_html(source):
        if len(row) < 6 or not row[0].lower().startswith("claude "):
            continue
        values = [money(cell) for cell in row[1:6]]
        if any(value is None for value in values):
            continue
        prices.append(
            Price(
                normalized(row[0]),
                values[0],
                values[3],
                values[4],
                cache_write=values[1],
                cache_write_1h=values[2],
            )
        )
    return unique_prices(prices)


def parse_deepseek(source: str) -> list[Price]:
    rows = rows_from_html(source)
    model_row = next(
        (row for row in rows if len(row) >= 3 and row[0].upper() == "MODEL"),
        None,
    )
    if model_row is None:
        return []
    models = [normalized(value) for value in model_row[1:3]]

    def off_peak_values(label: str) -> list[float] | None:
        row = next(
            (
                row
                for row in rows
                if label in " ".join(row).upper() and "OFF-PEAK" in " ".join(row).upper()
            ),
            None,
        )
        if row is None:
            return None
        values = [money(cell) for cell in row[-2:]]
        return values if all(value is not None for value in values) else None

    cache_hit = off_peak_values("CACHE HIT")
    cache_miss = off_peak_values("CACHE MISS")
    output = off_peak_values("1M OUTPUT")
    if not cache_hit or not cache_miss or not output or len(models) < 2:
        return []
    return [
        Price(
            models[0],
            cache_miss[0],
            cache_hit[0],
            output[0],
            cache_write=cache_miss[0],
        ),
        Price(
            models[1],
            cache_miss[1],
            cache_hit[1],
            output[1],
            cache_write=cache_miss[1],
        ),
    ]


def parse_kimi_page(source: str, wanted: Iterable[str]) -> list[Price]:
    source = html.unescape(source)
    prices: list[Price] = []
    for model in wanted:
        escaped = re.escape(model)
        match = re.search(
            rf"\[\[`{escaped}`.*?`¥([0-9]+(?:\.[0-9]+)?)`,`¥([0-9]+(?:\.[0-9]+)?)`,`¥([0-9]+(?:\.[0-9]+)?)`",
            source,
            re.S | re.I,
        )
        if match:
            prices.append(
                Price(
                    normalized(model),
                    float(match.group(2)),
                    float(match.group(1)),
                    float(match.group(3)),
                    currency="CNY",
                )
            )
    return prices


def parse_zai(source: str) -> list[Price]:
    prices: list[Price] = []
    for row in rows_from_html(source):
        if not row or not row[0].lower().startswith("glm") or len(row) < 5:
            continue
        input_price = money(row[1])
        cache_read = money(row[2])
        output = money(row[4])
        if input_price is not None and cache_read is not None and output is not None:
            prices.append(Price(normalized(row[0]), input_price, cache_read, output))
    return unique_prices(prices)


def parse_xai(source: str) -> list[Price]:
    source = html.unescape(source)
    prices: list[Price] = []
    pattern = re.compile(r'"name":"(grok-[^"]+)"(?P<body>.*?)(?="name":"|$)', re.S)
    for match in pattern.finditer(source):
        model = normalized(match.group(1))
        body = match.group("body")
        values = []
        for key in ["promptTextTokenPrice", "cachedPromptTokenPrice", "completionTextTokenPrice"]:
            value = re.search(rf'"{key}":"?([0-9]+(?:\.[0-9]+)?)', body)
            values.append(float(value.group(1)) / 10_000 if value else None)
        if all(value is not None for value in values):
            prices.append(Price(model, values[0], values[1], values[2]))
    return unique_prices(prices)


def parse_cursor(source: str) -> list[Price]:
    prices: list[Price] = []
    for row in rows_from_html(source):
        if not row or not row[0].lower().startswith("composer") or len(row) < 5:
            continue
        values = [money(cell) for cell in row[1:5]]
        if values[0] is not None and values[2] is not None and values[3] is not None:
            prices.append(Price(normalized(row[0]), values[0], values[2], values[3]))
    return unique_prices(prices)


def unique_prices(prices: list[Price]) -> list[Price]:
    result: dict[str, Price] = {}
    for price in prices:
        result.setdefault(price.name, price)
    return list(result.values())


ALIASES: dict[str, tuple[str, ...]] = {
    "gpt-5-6-sol": ("gpt-5-6",),
    "gpt-5-5-272k-context-length": ("gpt-5-5",),
    "gpt-5-4-272k-context-length": ("gpt-5-4",),
    "grok-build-0-1": ("grok-code-fast-1", "grok-code-fast", "grok-code-fast-1-0825"),
    "grok-4-5": ("grok-4-5-latest",),
    "grok-4-3": ("grok-4-3-latest",),
    "composer-2-5": ("composer-2.5",),
    "composer-2-5-fast": ("composer-2.5-fast",),
}


def with_aliases(prices: list[Price]) -> list[Price]:
    return [price.__class__(**{**price.__dict__, "aliases": ALIASES.get(price.name, ())}) for price in prices]


def fixture_path(fixture_dir: Path | None, name: str) -> Path | None:
    if fixture_dir is None:
        return None
    path = fixture_dir / name
    if not path.is_file():
        raise RuntimeError(f"missing fixture: {path}")
    return path


def build_catalog(fixture_dir: Path | None) -> tuple[list[Price], list[dict[str, object]]]:
    documents: dict[str, str] = {}
    for provider, url in SOURCES.items():
        documents[provider] = fetch(url, fixture_path(fixture_dir, f"{provider}.html"))

    prices: list[Price] = []
    prices.extend(parse_openai(documents["openai"]))
    prices.extend(parse_anthropic(documents["anthropic"]))
    prices.extend(parse_deepseek(documents["deepseek"]))
    prices.extend(parse_kimi_page(documents["kimi"], []))
    for model, url in KIMI_PAGES.items():
        prices.extend(parse_kimi_page(fetch(url, fixture_path(fixture_dir, f"{model}.html")), [model]))
    prices.extend(parse_zai(documents["zai"]))
    prices.extend(parse_xai(documents["xai"]))
    prices.extend(parse_cursor(documents["cursor"]))
    prices = with_aliases(unique_prices(prices))

    required = {"openai": 1, "anthropic": 1, "deepseek": 2, "kimi": 1, "zai": 1, "xai": 1, "cursor": 1}
    checks = {
        "openai": [price for price in prices if price.name.startswith(("gpt-", "o1", "o3", "o4"))],
        "anthropic": [price for price in prices if price.name.startswith("claude-")],
        "deepseek": [price for price in prices if price.name.startswith("deepseek-")],
        "kimi": [price for price in prices if price.currency == "CNY"],
        "zai": [price for price in prices if price.name.startswith("glm-")],
        "xai": [price for price in prices if price.name.startswith("grok-")],
        "cursor": [price for price in prices if price.name.startswith("composer-")],
    }
    for provider, minimum in required.items():
        if len(checks[provider]) < minimum:
            raise RuntimeError(f"{provider} pricing table parsed {len(checks[provider])} models; expected at least {minimum}")

    sources = []
    for provider, url in SOURCES.items():
        data = documents[provider].encode("utf-8")
        sources.append({"provider": provider, "url": url, "sha256": hashlib.sha256(data).hexdigest(), "modelCount": len(checks[provider])})
    return prices, sources


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--fixture-dir", type=Path, help="read provider HTML snapshots instead of using the network")
    args = parser.parse_args()
    try:
        prices, sources = build_catalog(args.fixture_dir)
        generated_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
        catalog = {
            "schemaVersion": 1,
            "generatedAt": generated_at,
            "sources": sources,
            "models": [price.as_json() for price in sorted(prices, key=lambda item: (item.currency, item.name))],
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"updated {args.output} with {len(prices)} models from {len(sources)} official sources")
        return 0
    except (OSError, RuntimeError, ValueError) as error:
        print(f"pricing update failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
