import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock


SPEC = importlib.util.spec_from_file_location("native_resources", Path(__file__).with_name("verify-resources.py"))
RESOURCES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESOURCES)


class NativeResourceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="native-resource-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.directory = self.root / RESOURCES.RESOURCE_DIRECTORY
        self.manifest = {"resourceVersion": 1, "resources": []}
        for index, (name, source) in enumerate(RESOURCES.SOURCES.items()):
            content = f"resource-{index}\r\n".encode()
            target = self.directory / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
            self.manifest["resources"].append({"path": name, "sourcePath": source, "size": len(content),
                                              "sha256": hashlib.sha256(content).hexdigest()})
        self.save()

    def save(self):
        (self.directory / "resources.json").write_text(json.dumps(self.manifest), encoding="utf-8")

    def references(self):
        for name, source in RESOURCES.SOURCES.items():
            path = self.root / source
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((self.directory / name).read_bytes())

    def test_native_verification_does_not_require_renderer_reference(self):
        self.assertFalse((self.root / "src").exists())
        result = RESOURCES.verify(self.root)
        self.assertTrue(result["passed"])
        self.assertEqual(len(result["resources"]), 4)
        self.assertFalse(result["referenceCompared"])
        self.assertFalse(result["nativeClientReady"])

    def test_reference_mode_compares_all_exact_source_bytes(self):
        self.references()
        self.assertTrue(RESOURCES.verify(self.root, reference=True)["referenceCompared"])
        (self.root / RESOURCES.SOURCES["catalogs/ru.json"]).write_bytes(b"different!\r\n")
        with self.assertRaises(ValueError):
            RESOURCES.verify(self.root, reference=True)

    def test_reference_mode_fails_when_source_is_absent(self):
        with self.assertRaises(FileNotFoundError):
            RESOURCES.verify(self.root, reference=True)

    def test_missing_native_model_fails_without_reference_fallback(self):
        (self.directory / "models/silero_vad.onnx").unlink()
        with self.assertRaises(FileNotFoundError):
            RESOURCES.verify(self.root)

    def test_corruption_and_eol_normalization_are_rejected(self):
        path = self.directory / "catalogs/ru.json"
        content = path.read_bytes()
        for damaged in [b"X" + content[1:], content.replace(b"\r\n", b"\n")]:
            with self.subTest(damaged=damaged):
                path.write_bytes(damaged)
                with self.assertRaises(ValueError):
                    RESOURCES.verify(self.root)
        path.write_bytes(content)

    def test_inventory_must_have_each_expected_resource_exactly_once(self):
        original = copy.deepcopy(self.manifest)
        variants = [original["resources"][:-1], original["resources"] + [original["resources"][0]],
                    [original["resources"][0]] * 4, [None] * 4]
        for records in variants:
            with self.subTest(records=records):
                self.manifest["resources"] = records
                self.save()
                with self.assertRaises(ValueError):
                    RESOURCES.verify(self.root)

    def test_paths_sizes_digests_and_version_are_fail_closed(self):
        original = copy.deepcopy(self.manifest)
        changes = [("path", "../../outside"), ("path", "C:/outside"), ("path", "catalogs\\ru.json"),
                   ("sourcePath", "../outside"), ("size", True), ("size", 0), ("size", 16 * 1024 * 1024 + 1),
                   ("sha256", "0" * 63), ("sha256", "G" * 64), ("sha256", None)]
        for field, value in changes:
            with self.subTest(field=field, value=value):
                self.manifest = copy.deepcopy(original)
                self.manifest["resources"][0][field] = value
                self.save()
                with self.assertRaises(ValueError):
                    RESOURCES.verify(self.root)
        for version in [True, 0, 2, "1"]:
            with self.subTest(version=version):
                self.manifest = copy.deepcopy(original)
                self.manifest["resourceVersion"] = version
                self.save()
                with self.assertRaises(ValueError):
                    RESOURCES.verify(self.root)

    def test_manifest_is_bounded_and_must_be_an_object(self):
        path = self.directory / "resources.json"
        for content in ["[]", "{}", "{" + " " * (64 * 1024) + "}"]:
            with self.subTest(length=len(content)):
                path.write_text(content, encoding="utf-8")
                with self.assertRaises(ValueError):
                    RESOURCES.verify(self.root)

    def test_directories_cannot_substitute_for_resources(self):
        path = self.directory / "models/silero_vad.onnx"
        path.unlink()
        path.mkdir()
        with self.assertRaises(ValueError):
            RESOURCES.verify(self.root)

    def test_repository_native_and_retained_reference_bytes_match(self):
        self.assertTrue(RESOURCES.verify(reference=True)["passed"])

    def test_redirected_resource_parent_is_rejected(self):
        original = Path.lstat
        redirected = self.directory / "catalogs"

        def lstat(path, *args, **kwargs):
            info = original(path, *args, **kwargs)
            if path == redirected:
                return SimpleNamespace(st_mode=info.st_mode, st_file_attributes=RESOURCES.stat.FILE_ATTRIBUTE_REPARSE_POINT)
            return info

        with mock.patch.object(Path, "lstat", lstat):
            with self.assertRaisesRegex(ValueError, "redirected"):
                RESOURCES.verify(self.root)


if __name__ == "__main__":
    unittest.main()
