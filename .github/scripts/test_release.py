"""Check release boundaries without building or publishing the application."""

import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile
from contextlib import redirect_stdout

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        original = release.ROOT
        self.addCleanup(setattr, release, "ROOT", original)
        self.root = release.ROOT = Path(self.temporary.name)
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "win-cap"\nversion = "0.1.1"\n', encoding="utf-8"
        )
        (self.root / "CHANGELOG.md").write_text(
            "# Changelog\n\n## [0.1.1]\n日本語の変更内容\n",
            encoding="utf-8",
        )

    def test_release_identity_comes_from_manifest(self):
        version, archive = release.release_info()
        self.assertEqual(version, "0.1.1")
        self.assertEqual(archive, "win-cap-0.1.1-windows-x64.zip")

    def test_check_exports_tag_and_archive_for_workflow(self):
        output = self.root / "github-output"
        with patch.object(release.os, "environ", {"GITHUB_OUTPUT": str(output)}), \
                patch.object(release.sys, "argv", ["release.py", "check"]), \
                redirect_stdout(io.StringIO()):
            release.main()
        self.assertEqual(output.read_text(encoding="utf-8"),
                         "version=0.1.1\ntag=v0.1.1\narchive=win-cap-0.1.1-windows-x64.zip\n")

    def test_nonstable_and_noncanonical_manifest_versions_are_rejected(self):
        for version in ("0.1.1-rc.1", "0.1.1+build", "00.1.1", "0.1", "０.1.1"):
            (self.root / "Cargo.toml").write_text(
                f'[package]\nversion = "{version}"\n', encoding="utf-8"
            )
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.release_info()

    def test_changelog_section_is_not_required_for_release(self):
        (self.root / "CHANGELOG.md").write_text("# Optional curated history\n", encoding="utf-8")
        self.assertEqual(release.release_info()[0], "0.1.1")

    def prepare_package(self):
        binary = self.root / "target/x86_64-pc-windows-msvc/release/win-cap.exe"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"MZ\x00test executable")
        for name in ("README.md", "LICENSE", "THIRD_PARTY_NOTICES.txt",
                     "docs/DEVELOPMENT.md", "docs/RELEASING.md"):
            source = self.root / name
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_text(f"Contents of {name}\n", encoding="utf-8")
        for name, contents in (("docs/assets/demo.gif", b"GIF89a demo"),
                               ("docs/assets/demo.mp4", b"MP4 demo")):
            source = self.root / name
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_bytes(contents)
        return binary

    def test_package_contents_and_checksum(self):
        binary = self.prepare_package()
        _, archive = release.release_info()
        release.package(archive)
        output = self.root / "dist"
        with zipfile.ZipFile(output / archive) as bundle:
            self.assertEqual(set(bundle.namelist()), {
                "win-cap.exe", "README.md", "CHANGELOG.md", "LICENSE",
                "THIRD_PARTY_NOTICES.txt",
                "docs/DEVELOPMENT.md", "docs/RELEASING.md",
                "docs/assets/demo.gif", "docs/assets/demo.mp4",
            })
            self.assertEqual(bundle.read("win-cap.exe"), binary.read_bytes())
            for name in ("docs/assets/demo.gif", "docs/assets/demo.mp4"):
                self.assertEqual(bundle.read(name), (self.root / name).read_bytes())
            self.assertEqual(bundle.testzip(), None)
        checksum = hashlib.sha256((output / archive).read_bytes()).hexdigest()
        self.assertEqual((output / f"{archive}.sha256").read_bytes(),
                         f"{checksum}  {archive}\n".encode())
        self.assertEqual(set(path.name for path in output.iterdir()), {archive, f"{archive}.sha256"})

    def test_missing_build_is_rejected_even_with_old_dist_binary(self):
        binary = self.prepare_package()
        binary.unlink()
        (self.root / "dist").mkdir()
        (self.root / "dist/win-cap.exe").write_bytes(b"old binary")
        _, archive = release.release_info()
        with self.assertRaisesRegex(ValueError, "Required package file is missing"):
            release.package(archive)
        self.assertFalse((self.root / "dist" / archive).exists())

    def test_existing_outputs_are_not_overwritten(self):
        self.prepare_package()
        _, archive = release.release_info()
        output = self.root / "dist"
        output.mkdir()
        for name in (archive, f"{archive}.sha256"):
            path = output / name
            path.write_bytes(b"existing output")
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "already exists"):
                release.package(archive)
            self.assertEqual(path.read_bytes(), b"existing output")
            path.unlink()


if __name__ == "__main__":
    unittest.main()
