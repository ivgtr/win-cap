"""Validate release metadata and package the Windows build using only the stdlib."""

import argparse
import hashlib
import os
from pathlib import Path
import re
import sys
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[2]
VERSION_PATTERN = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"


def release_info(tag: str | None) -> tuple[str, str, str]:
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["package"]["version"]
    if not isinstance(version, str) or not re.fullmatch(VERSION_PATTERN, version):
        raise ValueError("Cargo.toml version must be MAJOR.MINOR.PATCH (stable only).")
    if tag is not None and tag != f"v{version}":
        raise ValueError(f"Release tag must be v{version}; received {tag!r}.")

    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    headings = list(re.finditer(r"^## \[([^\]\n]+)\][^\n]*$", changelog, re.MULTILINE))
    matching = [i for i, heading in enumerate(headings) if heading[1] == version]
    if len(matching) != 1:
        raise ValueError(f"CHANGELOG.md must contain exactly one ## [{version}] section.")
    index = matching[0]
    start = headings[index].end()
    end = headings[index + 1].start() if index + 1 < len(headings) else len(changelog)
    notes = changelog[start:end].strip()
    if not notes:
        raise ValueError(f"CHANGELOG.md section [{version}] must not be empty.")
    return version, f"win-cap-{version}-windows-x64.zip", notes + "\n"


def package(archive: str, notes: str) -> None:
    files = {
        "win-cap.exe": ROOT / "target/x86_64-pc-windows-msvc/release/win-cap.exe",
        **{name: ROOT / name for name in (
            "README.md", "CHANGELOG.md", "LICENSE", "THIRD_PARTY_NOTICES.txt"
        )},
    }
    for source in files.values():
        if not source.is_file():
            raise ValueError(f"Required package file is missing: {source.relative_to(ROOT)}")
    output = ROOT / "dist"
    output.mkdir(exist_ok=True)
    paths = [output / archive, output / f"{archive}.sha256", output / "release-notes.md"]
    if any(path.exists() for path in paths):
        raise ValueError("Release output already exists in dist; remove those files before packaging.")
    with zipfile.ZipFile(paths[0], "x", compression=zipfile.ZIP_DEFLATED) as bundle:
        for name, source in files.items():
            bundle.write(source, arcname=name)
    with paths[0].open("rb") as bundle:
        checksum = hashlib.file_digest(bundle, "sha256").hexdigest()
    with paths[1].open("x", encoding="utf-8", newline="\n") as checksum_file:
        checksum_file.write(f"{checksum}  {archive}\n")
    with paths[2].open("x", encoding="utf-8", newline="\n") as notes_file:
        notes_file.write(notes)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("check")
    check.add_argument("--tag", help="Existing release tag; must equal v + Cargo.toml version")
    commands.add_parser("package")
    args = parser.parse_args()
    version, archive, notes = release_info(args.tag if args.command == "check" else None)
    if args.command == "package":
        package(archive, notes)
        print(f"Packaged dist/{archive}")
    else:
        result = f"version={version}\narchive={archive}\n"
        print(result, end="")
        if "GITHUB_OUTPUT" in os.environ:
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
                output.write(result)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, tomllib.TOMLDecodeError) as error:
        print(f"Release error: {error}", file=sys.stderr)
        sys.exit(1)
