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


def release_info() -> tuple[str, str]:
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["package"]["version"]
    if not isinstance(version, str) or not re.fullmatch(VERSION_PATTERN, version):
        raise ValueError("Cargo.toml version must be MAJOR.MINOR.PATCH (stable only).")
    return version, f"win-cap-{version}-windows-x64.zip"


def package(archive: str) -> None:
    files = {
        "win-cap.exe": ROOT / "target/x86_64-pc-windows-msvc/release/win-cap.exe",
        **{name: ROOT / name for name in (
            "README.md", "CHANGELOG.md", "LICENSE", "THIRD_PARTY_NOTICES.txt",
            "docs/DEVELOPMENT.md", "docs/RELEASING.md"
        )},
    }
    for source in files.values():
        if not source.is_file():
            raise ValueError(f"Required package file is missing: {source.relative_to(ROOT)}")
    output = ROOT / "dist"
    output.mkdir(exist_ok=True)
    paths = [output / archive, output / f"{archive}.sha256"]
    if any(path.exists() for path in paths):
        raise ValueError("Release output already exists in dist; remove those files before packaging.")
    with zipfile.ZipFile(paths[0], "x", compression=zipfile.ZIP_DEFLATED) as bundle:
        for name, source in files.items():
            bundle.write(source, arcname=name)
    with paths[0].open("rb") as bundle:
        checksum = hashlib.file_digest(bundle, "sha256").hexdigest()
    with paths[1].open("x", encoding="utf-8", newline="\n") as checksum_file:
        checksum_file.write(f"{checksum}  {archive}\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("check")
    commands.add_parser("package")
    args = parser.parse_args()
    version, archive = release_info()
    if args.command == "package":
        package(archive)
        print(f"Packaged dist/{archive}")
    else:
        result = f"version={version}\ntag=v{version}\narchive={archive}\n"
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
