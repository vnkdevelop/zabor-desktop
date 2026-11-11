import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("reference", Path(__file__).with_name("package-reference.py"))
REFERENCE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REFERENCE)


class PackageReferenceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="package-reference-test-", dir=Path.home() / "AppData/Local/Temp/opencode")
        self.root = Path(self.temporary.name)
        self.file = self.root / "node_modules/fixture/package.json"
        self.file.parent.mkdir(parents=True)
        self.file.write_bytes(b'{"name":"fixture","version":"1.0.0"}')
        self.manifest = {"packages": [{"path": "node_modules/fixture", "files": [
            {"path": "package.json", "size": self.file.stat().st_size,
             "sha256": REFERENCE.CLOSURE.sha256(self.file.read_bytes())}]}]}
        self.inventory = patch.object(REFERENCE, "inventory", return_value=self.manifest)
        self.inventory.start()

    def tearDown(self):
        self.inventory.stop()
        self.temporary.cleanup()

    def test_exact_files_do_not_grant_native_or_capture_acceptance(self):
        result = REFERENCE.check(self.root)
        self.assertEqual(result["files"], 1)
        self.assertEqual(result["packages"], 1)
        self.assertFalse(result["nativeClientReady"])
        self.assertFalse(result["captureAccepted"])

    def test_corrupted_file_fails(self):
        self.file.write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "file mismatch"):
            REFERENCE.check(self.root)

    def test_missing_file_fails(self):
        self.file.unlink()
        with self.assertRaises(FileNotFoundError):
            REFERENCE.check(self.root)

    def test_unlisted_files_and_empty_directories_fail(self):
        for extra in [self.root / "extra.js", self.root / "extra"]:
            if extra.suffix:
                extra.write_bytes(b"unlisted")
            else:
                extra.mkdir()
            with self.assertRaisesRegex(ValueError, "Unexpected reference package path"):
                REFERENCE.check(self.root)
            if extra.is_dir():
                extra.rmdir()
            else:
                extra.unlink()

    def test_existing_destination_is_never_overwritten(self):
        with patch.object(REFERENCE, "DESTINATION", self.root):
            with self.assertRaisesRegex(ValueError, "Existing package reference preserved"):
                REFERENCE.prepare()
        self.assertTrue(self.file.exists())

    def test_hard_linked_reference_is_rejected_even_when_bytes_match(self):
        original = self.root / "original.json"
        self.file.rename(original)
        os.link(original, self.file)
        with self.assertRaisesRegex(ValueError, "hard link|Unexpected"):
            REFERENCE.check(self.root)

    def test_modified_inventory_pin_fails_before_dependency_resolution(self):
        self.inventory.stop()
        with patch.object(REFERENCE, "PIN", "0" * 64):
            with self.assertRaisesRegex(ValueError, "inventory pin mismatch"):
                REFERENCE.inventory()
        self.inventory.start()


if __name__ == "__main__":
    unittest.main()
