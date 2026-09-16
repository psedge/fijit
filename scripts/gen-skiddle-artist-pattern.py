#!/usr/bin/env python3
"""Regenerate the ARTIST_PATTERN var in a Skiddle scraper's TOML file from the
artist names found in ~/hack/spotify/playlists/*.json.

Run this whenever your playlists change:

    python3 scripts/gen-skiddle-artist-pattern.py scrapers/skiddle-clubs-london.toml

It replaces the existing `ARTIST_PATTERN = '''...'''` line in place; every
other line in the file (comments, steps, state) is left untouched.
"""
import glob
import json
import re
import sys
from pathlib import Path

PLAYLISTS_DIR = Path.home() / "hack" / "spotify" / "playlists"

REGEX_META = set(r"\^$.|?*+()[]{}")


def escape(name: str) -> str:
    return "".join(f"\\{c}" if c in REGEX_META else c for c in name)


def collect_artists() -> set[str]:
    artists = set()
    for path in glob.glob(str(PLAYLISTS_DIR / "*.json")):
        if path.endswith("_manifest.json"):
            continue
        data = json.loads(Path(path).read_text())
        for track in data.get("tracks", []):
            for artist in track.get("artists", []):
                artist = artist.strip()
                if artist:
                    artists.add(artist)
    return artists


def build_pattern(artists: set[str]) -> str:
    escaped = sorted((escape(a) for a in artists), key=len, reverse=True)
    return r"(?i)\b(?:" + "|".join(escaped) + r")\b"


def main() -> None:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <scraper.toml>", file=sys.stderr)
        sys.exit(1)

    scraper_path = Path(sys.argv[1])
    artists = collect_artists()
    pattern = build_pattern(artists)

    text = scraper_path.read_text()
    marker = re.search(r"^ARTIST_PATTERN = '''.*?'''$", text, flags=re.M | re.S)
    if marker is None:
        print(
            "no ARTIST_PATTERN line found to replace -- add one under [vars] first",
            file=sys.stderr,
        )
        sys.exit(1)

    new_line = f"ARTIST_PATTERN = '''{pattern}'''"
    updated = text[: marker.start()] + new_line + text[marker.end() :]
    scraper_path.write_text(updated)
    print(f"wrote pattern for {len(artists)} artists ({len(pattern)} chars) into {scraper_path}")


if __name__ == "__main__":
    main()
