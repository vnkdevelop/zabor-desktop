import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile


SPEC = importlib.util.spec_from_file_location("electron_reference", Path(__file__).with_name("electron-reference.py"))
ELECTRON = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ELECTRON)
FIXTURE_SPEC = importlib.util.spec_from_file_location("package_fixture", Path(__file__).parent / "packaging/test_package.py")
FIXTURE = importlib.util.module_from_spec(FIXTURE_SPEC)
FIXTURE_SPEC.loader.exec_module(FIXTURE)


class ElectronReferenceTests(unittest.TestCase):
    def setUp(self):
        self.owner = tempfile.TemporaryDirectory(prefix="electron-reference-fixture-", dir=Path.home() / "AppData/Local/Temp/opencode")
        self.folder = Path(self.owner.name)

    def tearDown(self):
        self.owner.cleanup()

    def fixture(self, changes=None, additional=()):
        entries = {"electron.exe": FIXTURE.fixture_pe("kernel32.dll"), "version": b"44.4.3",
                   "LICENSE": b"LOCAL TEST NOTICE ONLY", "LICENSES.chromium.html": b"LOCAL TEST NOTICE ONLY"}
        if changes:
            changes(entries)
        path = self.folder / "fixture.zip"
        with zipfile.ZipFile(path, "w") as archive:
            for name, raw in entries.items():
                archive.writestr(name, raw)
            for name in additional:
                archive.writestr(name, b"LOCAL TEST ONLY")
        return path, hashlib.sha256(path.read_bytes()).hexdigest()

    def test_real_official_runtime_archive_is_integrity_only(self):
        result = ELECTRON.check()
        self.assertEqual(result["runtimeVersion"], "44.4.3")
        self.assertEqual(result["files"], 73)
        self.assertFalse(result["captureAccepted"])

    def test_fixture_roundtrip_and_corrupt_archive_hash(self):
        path, digest = self.fixture()
        with patch.object(ELECTRON, "HASH", digest):
            self.assertEqual(len(ELECTRON.inspect(path)), 4)
            with path.open("ab") as stream:
                stream.write(b"corrupt")
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                ELECTRON.inspect(path)

    def test_missing_original_notice_or_wrong_version_fails(self):
        for change, reason in [(lambda data: data.pop("LICENSES.chromium.html"), "Incomplete"),
                               (lambda data: data.update(version=b"1.0.0"), "version")]:
            path, digest = self.fixture(change)
            with patch.object(ELECTRON, "HASH", digest):
                with self.assertRaisesRegex(ValueError, reason):
                    ELECTRON.inspect(path)

    def test_non_native_managed_reference_is_rejected(self):
        path, digest = self.fixture(lambda data: data.update({"electron.exe": FIXTURE.fixture_pe(managed=True)}))
        with patch.object(ELECTRON, "HASH", digest):
            with self.assertRaisesRegex(ValueError, "CLR"):
                ELECTRON.inspect(path)

    def test_zip_path_aliases_and_case_collision_fail(self):
        for name, reason in [("../escape", "Unsafe"), ("C:/absolute", "Unsafe"),
                             ("file:stream", "Unsafe"), ("trailing.", "Unsafe"), ("ELECTRON.EXE", "collision")]:
            path, digest = self.fixture(additional=[name])
            with patch.object(ELECTRON, "HASH", digest):
                with self.assertRaisesRegex(ValueError, reason):
                    ELECTRON.inspect(path)

    def test_zip_symlink_is_rejected(self):
        path, _ = self.fixture()
        with zipfile.ZipFile(path, "a") as archive:
            link = zipfile.ZipInfo("link")
            link.external_attr = 0o120777 << 16
            archive.writestr(link, "electron.exe")
        with patch.object(ELECTRON, "HASH", hashlib.sha256(path.read_bytes()).hexdigest()):
            with self.assertRaisesRegex(ValueError, "Unsafe"):
                ELECTRON.inspect(path)


if __name__ == "__main__":
    unittest.main()
