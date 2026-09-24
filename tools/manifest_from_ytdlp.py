import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description="Build a Music Library import manifest from yt-dlp sidecars")
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    directory = args.directory.resolve()
    tracks = []
    for sidecar in sorted(directory.glob("*.info.json")):
        audio = sidecar.with_name(sidecar.name.removesuffix(".info.json") + ".mp3")
        if not audio.is_file():
            print(f"Skipping {sidecar.name}: matching MP3 is missing")
            continue
        info = json.loads(sidecar.read_text(encoding="utf-8"))
        tracks.append({
            "file": audio.name,
            "title": info.get("track") or info.get("title") or audio.stem,
            "artist": info.get("artist") or info.get("creator") or "",
            "album": info.get("album") or "",
        })
    output = directory / "music-library-import.json"
    output.write_text(json.dumps({"version": 1, "tracks": tracks}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {len(tracks)} tracks to {output}")


if __name__ == "__main__":
    main()
