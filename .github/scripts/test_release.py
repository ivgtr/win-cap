"""Check release boundaries without building or publishing the application."""

import hashlib
from pathlib import Path
import tempfile
import unittest
import zipfile

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
            "# Changelog\n\n## [Unreleased]\nFuture changes\n"
            "\n## [0.1.1]\n日本語の変更内容\n\n## [0.1.0]\nOlder changes\n",
            encoding="utf-8",
        )

    def test_matching_tag_uses_only_current_notes(self):
        version, archive, notes = release.release_info("v0.1.1")
        self.assertEqual(version, "0.1.1")
        self.assertEqual(archive, "win-cap-0.1.1-windows-x64.zip")
        self.assertEqual(notes, "日本語の変更内容\n")

    def test_wrong_and_nonstable_tags_are_rejected(self):
        for tag in ("v0.1.2", "0.1.1", "v0.1.1-rc.1", "v0.1.1+build", ""):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.release_info(tag)

    def test_nonstable_and_noncanonical_manifest_versions_are_rejected(self):
        for version in ("0.1.1-rc.1", "0.1.1+build", "00.1.1", "0.1", "０.1.1"):
            (self.root / "Cargo.toml").write_text(
                f'[package]\nversion = "{version}"\n', encoding="utf-8"
            )
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.release_info(None)

    def test_missing_duplicate_and_empty_changelog_sections_are_rejected(self):
        for changelog in ("## [0.1.0]\nOld\n", "## [0.1.1]\n",
                          "## [0.1.1]\nFirst\n## [0.1.1]\nSecond\n"):
            (self.root / "CHANGELOG.md").write_text(changelog, encoding="utf-8")
            with self.subTest(changelog=changelog), self.assertRaises(ValueError):
                release.release_info(None)

    def prepare_package(self):
        binary = self.root / "target/x86_64-pc-windows-msvc/release/win-cap.exe"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"MZ\x00test executable")
        for name in ("README.md", "LICENSE", "THIRD_PARTY_NOTICES.txt"):
            (self.root / name).write_text(f"Contents of {name}\n", encoding="utf-8")
        return binary

    def test_package_contents_checksum_and_notes(self):
        binary = self.prepare_package()
        _, archive, notes = release.release_info("v0.1.1")
        release.package(archive, notes)
        output = self.root / "dist"
        with zipfile.ZipFile(output / archive) as bundle:
            self.assertEqual(set(bundle.namelist()), {
                "win-cap.exe", "README.md", "CHANGELOG.md", "LICENSE",
                "THIRD_PARTY_NOTICES.txt",
            })
            self.assertEqual(bundle.read("win-cap.exe"), binary.read_bytes())
            self.assertEqual(bundle.testzip(), None)
        checksum = hashlib.sha256((output / archive).read_bytes()).hexdigest()
        self.assertEqual((output / f"{archive}.sha256").read_bytes(),
                         f"{checksum}  {archive}\n".encode())
        self.assertEqual((output / "release-notes.md").read_bytes(), notes.encode())

    def test_missing_build_is_rejected_even_with_old_dist_binary(self):
        binary = self.prepare_package()
        binary.unlink()
        (self.root / "dist").mkdir()
        (self.root / "dist/win-cap.exe").write_bytes(b"old binary")
        _, archive, notes = release.release_info(None)
        with self.assertRaisesRegex(ValueError, "Required package file is missing"):
            release.package(archive, notes)
        self.assertFalse((self.root / "dist" / archive).exists())

    def test_existing_outputs_are_not_overwritten(self):
        self.prepare_package()
        _, archive, notes = release.release_info(None)
        output = self.root / "dist"
        output.mkdir()
        for name in (archive, f"{archive}.sha256", "release-notes.md"):
            path = output / name
            path.write_bytes(b"existing output")
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "already exists"):
                release.package(archive, notes)
            self.assertEqual(path.read_bytes(), b"existing output")
            path.unlink()


if __name__ == "__main__":
    unittest.main()
