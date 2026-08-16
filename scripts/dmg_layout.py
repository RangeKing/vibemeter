#!/usr/bin/env python3
"""Write and verify the Finder layout for VibeMeter DMG support files."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import stat
import subprocess
import sys

import ds_store.store as ds_store_module
from ds_store import DSStore


SUPPORT_ITEMS = (".background", ".VolumeIcon.icns")
DEFAULT_WINDOW_WIDTH = 860


def _disable_bookmark_decoding() -> None:
    # Finder writes a pBBk bookmark variant that ds_store 1.3.1 cannot decode.
    # It is unrelated to icon locations, so preserve it as an opaque blob.
    ds_store_module.codecs.pop(b"pBBk", None)


def _positions(window_width: int) -> dict[str, tuple[int, int]]:
    hidden_x = window_width + 200
    return {
        ".background": (hidden_x, 100),
        ".VolumeIcon.icns": (hidden_x, 180),
    }


def _store_path(mount_point: Path) -> Path:
    path = mount_point / ".DS_Store"
    if not path.is_file():
        raise RuntimeError(f"DMG is missing Finder layout metadata: {path}")
    return path


def finalize(mount_point: Path, window_width: int) -> None:
    positions = _positions(window_width)
    with DSStore.open(os.fspath(_store_path(mount_point)), "r+") as store:
        for item, position in positions.items():
            if not (mount_point / item).exists():
                raise RuntimeError(f"DMG is missing support item: {item}")
            store[item]["Iloc"] = position
            print(f"positioned {item} at {position[0]},{position[1]}")


def _finder_invisible(path: Path) -> bool:
    result = subprocess.run(
        ["/usr/bin/GetFileInfo", "-av", os.fspath(path)],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip() == "1"


def verify(mount_point: Path, window_width: int) -> None:
    failures: list[str] = []
    positions: dict[str, tuple[int, int]] = {}
    with DSStore.open(os.fspath(_store_path(mount_point)), "r") as store:
        for item in SUPPORT_ITEMS:
            path = mount_point / item
            if not path.exists():
                failures.append(f"missing support item: {item}")
                continue

            try:
                position = store[item]["Iloc"]
                positions[item] = position
            except KeyError:
                failures.append(f"missing off-window icon position: {item}")
                continue

            if position[0] <= window_width:
                failures.append(
                    f"visible icon position for {item}: {position[0]},{position[1]}"
                )

            flags = path.stat().st_flags
            if not flags & stat.UF_HIDDEN:
                failures.append(f"missing POSIX hidden flag: {item}")
            if not _finder_invisible(path):
                failures.append(f"missing Finder invisible flag: {item}")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}", file=sys.stderr)
        raise SystemExit(1)

    for item in SUPPORT_ITEMS:
        position = positions[item]
        print(
            f"PASS: {item} is hidden and positioned at "
            f"{position[0]},{position[1]}"
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("finalize", "verify"))
    parser.add_argument("mount_point", type=Path)
    parser.add_argument(
        "--window-width", type=int, default=DEFAULT_WINDOW_WIDTH
    )
    args = parser.parse_args()

    _disable_bookmark_decoding()
    mount_point = args.mount_point.resolve()
    if not mount_point.is_dir():
        parser.error(f"mount point does not exist: {mount_point}")

    if args.command == "finalize":
        finalize(mount_point, args.window_width)
    else:
        verify(mount_point, args.window_width)


if __name__ == "__main__":
    main()
