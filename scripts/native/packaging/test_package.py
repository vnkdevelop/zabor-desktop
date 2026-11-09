import importlib.util
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile


sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("native_package", Path(__file__).with_name("package.py"))
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)
AUTH_SPEC = importlib.util.spec_from_file_location("native_authenticode", Path(__file__).with_name("authenticode.py"))
AUTH = importlib.util.module_from_spec(AUTH_SPEC)
AUTH_SPEC.loader.exec_module(AUTH)


def fixture_pe(import_name=None, managed=False):
    data = bytearray(1024)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 128)
    data[128:132] = b"PE\0\0"
    struct.pack_into("<HH", data, 132, 0x8664, 1)
    struct.pack_into("<H", data, 148, 240)
    struct.pack_into("<H", data, 152, 0x20B)
    struct.pack_into("<IIII", data, 400, 512, 4096, 512, 512)
    if import_name:
        struct.pack_into("<II", data, 152 + 112 + 8, 4096, 40)
        struct.pack_into("<I", data, 512 + 12, 4196)
        name = import_name.encode() + b"\0"
        data[612:612 + len(name)] = name
    if managed:
        struct.pack_into("<II", data, 152 + 112 + 14 * 8, 4096, 72)
    return data


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix=".native-package-fixture-", dir=PACKAGE.ROOT)
        self.root = Path(self.temporary.name)
        self.stage = self.root / "stage"
        self.stage.mkdir()
        (self.stage / "zabor-native.exe").write_bytes(fixture_pe("kernel32.dll"))
        license_path = self.stage / "licenses/fixture/LICENSE"
        license_path.parent.mkdir(parents=True)
        license_path.write_text("LOCAL TEST FIXTURE ONLY; not a distributable application", encoding="utf-8")
        PACKAGE.write_json(self.stage / "licenses/inventory.json", {
            "inventoryVersion": 1, "target": PACKAGE.POLICY["target"], "complete": True,
            "packages": [{"name": "local-test-fixture", "licenseExpression": "LOCAL-TEST-ONLY",
                          "texts": [{"path": "licenses/fixture/LICENSE", "sha256": PACKAGE.digest(license_path)}]}]})

    def tearDown(self):
        self.temporary.cleanup()

    def archive(self):
        path = self.root / "fixture.zip"
        result = PACKAGE.make_archive(self.stage, path, neural=False, fixture=True)
        self.assertFalse(result["installationAllowed"])
        self.assertFalse(result["nativeClientReady"])
        return path

    def rewrite(self, path, mutate):
        with zipfile.ZipFile(path) as archive:
            entries = {name: archive.read(name) for name in archive.namelist()}
        mutate(entries)
        path.unlink()
        with zipfile.ZipFile(path, "w") as archive:
            for name, data in entries.items():
                archive.writestr(name, data)

    def test_local_package_roundtrip_never_grants_installation(self):
        result = PACKAGE.verify_archive(self.archive())
        self.assertEqual(result["localIntegrity"], "passed")
        self.assertFalse(result["installationAllowed"])
        self.assertEqual(result["peImports"]["zabor-native.exe"], ["kernel32.dll"])

    def test_no_overwrite(self):
        path = self.archive()
        before = PACKAGE.digest(path)
        with self.assertRaisesRegex(ValueError, "overwrite"):
            PACKAGE.make_archive(self.stage, path, False, True)
        self.assertEqual(PACKAGE.digest(path), before)

    def test_native_helper_is_exactly_allowlisted_and_hashed(self):
        helper = self.stage / PACKAGE.POLICY["updateHelper"]
        helper.write_bytes(fixture_pe("kernel32.dll"))
        path = self.archive()
        result = PACKAGE.verify_archive(path)
        self.assertEqual(result["peImports"][helper.name], ["kernel32.dll"])
        self.rewrite(path, lambda entries: entries.update({helper.name: b"tampered"}))
        with self.assertRaisesRegex(ValueError, "hash/length"):
            PACKAGE.verify_archive(path)

    def test_non_fixture_requires_original_native_helper(self):
        with self.assertRaisesRegex(ValueError, "missing native binaries"):
            PACKAGE.make_archive(self.stage, self.root / "not-live.zip", False, False)

    def test_unsigned_helper_or_missing_pin_cannot_enter_certified_route(self):
        source = self.root / "unsigned-helper.exe"
        source.write_bytes(fixture_pe("kernel32.dll"))
        with self.assertRaisesRegex(ValueError, "trusted update helper certificate"):
            PACKAGE.copy_certified_update_helper(source, self.stage, "")
        with self.assertRaises(RuntimeError):
            PACKAGE.copy_certified_update_helper(source, self.stage, "1" * 64)

    def test_managed_or_dynamic_crt_helper_is_rejected(self):
        source = self.root / "invalid-helper.exe"
        for data, reason in [(fixture_pe(managed=True), "CLR"),
                             (fixture_pe("vcruntime140.dll"), "static CRT")]:
            source.write_bytes(data)
            (self.stage / PACKAGE.POLICY["updateHelper"]).unlink(missing_ok=True)
            with self.assertRaisesRegex(ValueError, reason):
                PACKAGE.copy_certified_update_helper(source, self.stage, "1" * 64)

    def test_followup_source_flip_cannot_certify_staged_non_system_imports(self):
        source = self.root / "flipped-helper.exe"
        source.write_bytes(fixture_pe("kernel32.dll"))
        copy = PACKAGE.shutil.copyfile

        def flip_before_copy(original, target):
            original.write_bytes(fixture_pe("foreign-helper.dll"))
            return copy(original, target)

        with patch.object(PACKAGE.shutil, "copyfile", side_effect=flip_before_copy), patch.object(PACKAGE, "run") as verify:
            with self.assertRaisesRegex(ValueError, "static CRT and system-only"):
                PACKAGE.copy_certified_update_helper(source, self.stage, "1" * 64)
            verify.assert_not_called()

    def test_followup_staged_bytes_and_namespace_are_held_during_certificate_check(self):
        source = self.root / "held-helper.exe"
        source.write_bytes(fixture_pe("kernel32.dll"))
        target = self.stage / PACKAGE.POLICY["updateHelper"]

        def verify(parts):
            self.assertEqual(parts[3], target)
            with self.assertRaises(OSError):
                target.write_bytes(fixture_pe("foreign-helper.dll"))
            with self.assertRaises(OSError):
                target.rename(self.stage / "replacement.exe")
            with self.assertRaises(OSError):
                self.stage.rename(self.root / "replacement-stage")
            return json.dumps({"authenticode": "trusted", "certificateSha256": "1" * 64})

        with patch.object(PACKAGE, "run", side_effect=verify):
            receipt = PACKAGE.copy_certified_update_helper(source, self.stage, "1" * 64)
        self.assertEqual(receipt["sha256"], PACKAGE.digest(target))
        result = PACKAGE.verify_archive(self.archive())
        self.assertFalse(result["installationAllowed"])
        self.assertEqual(result["peImports"][target.name], ["kernel32.dll"])

    def test_followup_changed_certified_helper_cannot_enter_composition(self):
        source = self.root / "composition-helper.exe"
        source.write_bytes(fixture_pe("kernel32.dll"))
        with patch.object(PACKAGE, "run", return_value=json.dumps({"authenticode": "trusted", "certificateSha256": "1" * 64})):
            receipt = PACKAGE.copy_certified_update_helper(source, self.stage, "1" * 64)
        (self.stage / PACKAGE.POLICY["updateHelper"]).write_bytes(fixture_pe("foreign-helper.dll"))
        with patch.object(PACKAGE, "make_archive") as compose:
            with self.assertRaisesRegex(ValueError, "bytes changed before composition"):
                PACKAGE.archive_with_certified_update_helper(self.stage, self.root / "not-live.zip", False, receipt)
            compose.assert_not_called()

    def test_followup_composition_keeps_certified_helper_bytes_held(self):
        source = self.root / "composition-held-helper.exe"
        source.write_bytes(fixture_pe("kernel32.dll"))
        target = self.stage / PACKAGE.POLICY["updateHelper"]
        with patch.object(PACKAGE, "run", return_value=json.dumps({"authenticode": "trusted", "certificateSha256": "1" * 64})):
            receipt = PACKAGE.copy_certified_update_helper(source, self.stage, "1" * 64)
        archive = PACKAGE.make_archive

        def compose(stage, output, neural):
            with self.assertRaises(OSError):
                target.write_bytes(fixture_pe("foreign-helper.dll"))
            return archive(stage, output, neural, fixture=True)

        path = self.root / "held-composition.zip"
        with patch.object(PACKAGE, "make_archive", side_effect=compose):
            result = PACKAGE.archive_with_certified_update_helper(self.stage, path, False, receipt)
        self.assertFalse(result["installationAllowed"])
        with zipfile.ZipFile(path) as payload:
            metadata = json.loads(payload.read("native-package.json"))
        entry = next(item for item in metadata["files"] if item["path"] == target.name)
        self.assertEqual(entry["sha256"], receipt["sha256"])

    def test_tampered_binary_rejected(self):
        path = self.archive()
        self.rewrite(path, lambda entries: entries.update({"zabor-native.exe": b"tampered"}))
        with self.assertRaisesRegex(ValueError, "hash/length"):
            PACKAGE.verify_archive(path)

    def test_unlisted_file_rejected(self):
        path = self.archive()
        self.rewrite(path, lambda entries: entries.update({"unexpected.txt": b"extra"}))
        with self.assertRaisesRegex(ValueError, "Unlisted"):
            PACKAGE.verify_archive(path)

    def test_path_traversal_and_windows_aliases_rejected(self):
        for name in ["../x", "C:/x", "\\x", "a//b", "a/./b", "a/../b", "a:stream", "CON.txt",
                     "a/NUL", "COM1.exe", "dir/LPT9", "a.", "a "]:
            self.assertFalse(PACKAGE.safe_name(name), name)
        self.assertTrue(PACKAGE.safe_name("licenses/package-1.2.0/LICENSE-MIT"))

    def test_case_collision_rejected(self):
        path = self.archive()
        self.rewrite(path, lambda entries: entries.update({"ZABOR-NATIVE.EXE": b"collision"}))
        with self.assertRaisesRegex(ValueError, "Unsafe"):
            PACKAGE.verify_archive(path)

    def test_symlink_entry_rejected(self):
        path = self.archive()
        with zipfile.ZipFile(path, "a") as archive:
            info = zipfile.ZipInfo("link")
            info.external_attr = 0o120777 << 16
            archive.writestr(info, "target")
        with self.assertRaisesRegex(ValueError, "Unsafe"):
            PACKAGE.verify_archive(path)

    def test_electron_and_managed_pe_rejected(self):
        (self.stage / "electron.exe").write_bytes(fixture_pe())
        with self.assertRaisesRegex(ValueError, "native payload"):
            PACKAGE.make_archive(self.stage, self.root / "bad.zip", False, True)
        with self.assertRaisesRegex(ValueError, "CLR"):
            PACKAGE.inspect_pe(fixture_pe(managed=True))

    def test_non_system_dependency_rejected(self):
        (self.stage / "zabor-native.exe").write_bytes(fixture_pe("vcruntime140.dll"))
        with self.assertRaisesRegex(ValueError, "Unbundled"):
            PACKAGE.make_archive(self.stage, self.root / "bad.zip", False, True)

    def test_wrong_architecture_and_truncated_pe_rejected(self):
        data = fixture_pe()
        struct.pack_into("<H", data, 132, 0x14C)
        with self.assertRaisesRegex(ValueError, "AMD64"):
            PACKAGE.inspect_pe(data)
        with self.assertRaisesRegex(ValueError, "Truncated"):
            PACKAGE.inspect_pe(fixture_pe()[:300])

    def test_license_bytes_are_verified_independently(self):
        inventory = self.stage / "licenses/inventory.json"
        data = json.loads(inventory.read_text())
        data["packages"][0]["texts"][0]["sha256"] = "0" * 64
        PACKAGE.write_json(inventory, data)
        with self.assertRaisesRegex(ValueError, "License inventory hash"):
            PACKAGE.make_archive(self.stage, self.root / "bad.zip", False, True)

    def test_onnx_version_not_silently_substituted(self):
        for name in ["onnxruntime.dll", "onnxruntime_providers_shared.dll"]:
            (self.stage / name).write_bytes(fixture_pe())
        with self.assertRaisesRegex(ValueError, "exactly 1.31.1"):
            PACKAGE.make_archive(self.stage, self.root / "bad.zip", True, True)

    def test_unsigned_fixture_cannot_pass_authenticode(self):
        with self.assertRaisesRegex(ValueError, "Authenticode trust failed closed"):
            AUTH.verify_authenticode(self.stage / "zabor-native.exe", "1" * 64)

    def test_missing_or_zero_publisher_pin_is_rejected_before_wintrust(self):
        for pin in ["", "0" * 64, "1" * 63, "not-a-certificate-pin"]:
            with self.assertRaisesRegex(ValueError, "explicit trusted publisher"):
                AUTH.verify_authenticode(self.stage / "zabor-native.exe", pin)

    def license_fixture(self):
        cache = self.root / "cache"
        for name in ["webrtc-win-x64-a545de4/LICENSE.md", "webrtc-win-x64-a545de4/lib/webrtc.lib",
                     "libclang-win-x64-18.1.1/licenses/LICENSE-0.txt"]:
            path = cache / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("LOCAL LICENSING FIXTURE ONLY", encoding="utf-8")
        sysroot = self.root / "toolchain"
        path = sysroot / "share/doc/rust/licenses/LICENSE-MIT"
        path.parent.mkdir(parents=True)
        path.write_text("LOCAL RUST LICENSING FIXTURE ONLY", encoding="utf-8")
        packages = []
        for name in ["zabor-native", "missing-upstream-notice"]:
            folder = self.root / name
            folder.mkdir()
            (folder / "Cargo.toml").write_text("LOCAL MANIFEST FIXTURE ONLY", encoding="utf-8")
            if name == "zabor-native":
                (folder / "LICENSE").write_text("LOCAL LICENSE FIXTURE ONLY", encoding="utf-8")
            packages.append({"id": name, "name": name, "version": "0.1.0", "source": None,
                             "license": "MIT", "manifest_path": str(folder / "Cargo.toml")})
        data = {"packages": packages, "resolve": {"nodes": [
            {"id": "zabor-native", "deps": [{"pkg": "missing-upstream-notice", "dep_kinds": [{"kind": None}]}]},
            {"id": "missing-upstream-notice", "deps": []}]}}
        return cache, sysroot, data

    def test_missing_upstream_license_text_remains_explicitly_incomplete(self):
        cache, sysroot, data = self.license_fixture()
        output = self.root / "inventory"
        with patch.object(PACKAGE, "run", side_effect=[str(sysroot), "LOCAL TOOLCHAIN FIXTURE"]):
            PACKAGE.collect_licenses(output, data, cache, False)
        inventory = json.loads((output / "inventory.json").read_text())
        self.assertFalse(inventory["complete"])
        self.assertFalse(inventory["legalApproval"])
        self.assertEqual(inventory["missingUpstreamTexts"], ["missing-upstream-notice@0.1.0"])

    def test_local_license_supplement_requires_exact_pinned_bytes(self):
        cache, sysroot, data = self.license_fixture()
        notice = self.root / "UPSTREAM-FIXTURE-LICENSE"
        notice.write_text("LOCAL LICENSE SUPPLEMENT FIXTURE ONLY", encoding="utf-8")
        supplement = self.root / "supplement.json"
        PACKAGE.write_json(supplement, {"supplementVersion": 1, "texts": [{
            "package": "missing-upstream-notice", "version": "0.1.0", "file": notice.name,
            "source": "https://github.com/local-test-fixture/local-test-fixture/blob/1234567890abcdef1234567890abcdef12345678/LICENSE",
            "sha256": PACKAGE.digest(notice)}]})
        output = self.root / "inventory"
        with patch.object(PACKAGE, "run", side_effect=[str(sysroot), "LOCAL TOOLCHAIN FIXTURE"]):
            PACKAGE.collect_licenses(output, data, cache, False, supplement)
        inventory = json.loads((output / "inventory.json").read_text())
        self.assertTrue(inventory["complete"])
        self.assertFalse(inventory["legalApproval"])
        notice.write_text("TAMPERED LOCAL FIXTURE", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "exact local bytes"):
            PACKAGE.collect_licenses(self.root / "rejected", data, cache, False, supplement)

    def test_licensing_dev_only_dependencies_are_not_in_runtime_build_closure(self):
        cache, sysroot, data = self.license_fixture()
        data["resolve"]["nodes"][0]["deps"][0]["dep_kinds"][0]["kind"] = "dev"
        output = self.root / "inventory"
        with patch.object(PACKAGE, "run", side_effect=[str(sysroot), "LOCAL TOOLCHAIN FIXTURE"]):
            PACKAGE.collect_licenses(output, data, cache, False)
        inventory = json.loads((output / "inventory.json").read_text())
        self.assertNotIn("missing-upstream-notice", [item["name"] for item in inventory["packages"]])
        self.assertTrue(inventory["complete"])

    def test_model_policy_matches_native_owned_resource_bytes(self):
        resources = json.loads((PACKAGE.ROOT / "rust/crates/zabor-native/assets/resources.json").read_text())
        models = {Path(item["path"]).name: item["sha256"] for item in resources["resources"]
                  if item["path"].startswith("models/")}
        self.assertEqual(PACKAGE.POLICY["modelFiles"], models)
        for name, expected in models.items():
            self.assertEqual(PACKAGE.model_digest(name), expected)

    def test_native_model_hash_rejects_corruption_missing_bytes_and_renderer_fallback(self):
        name = "silero_vad.onnx"
        native = self.root / "rust/crates/zabor-native/assets/models" / name
        native.parent.mkdir(parents=True)
        original = PACKAGE.ROOT / "rust/crates/zabor-native/assets/models" / name
        native.write_bytes(original.read_bytes())
        reference = self.root / "src/renderer/public" / name
        reference.parent.mkdir(parents=True)
        reference.write_bytes(native.read_bytes())
        with patch.object(PACKAGE, "ROOT", self.root):
            self.assertEqual(PACKAGE.model_digest(name), PACKAGE.POLICY["modelFiles"][name])
            native.write_bytes(b"CORRUPT LOCAL MODEL FIXTURE")
            with self.assertRaisesRegex(ValueError, "Pinned native model hash mismatch"):
                PACKAGE.model_digest(name)
            native.unlink()
            with self.assertRaises(FileNotFoundError):
                PACKAGE.model_digest(name)

    def test_neural_license_inventory_uses_pinned_native_models_without_granting_approval(self):
        cache, sysroot, data = self.license_fixture()
        folder = self.root / "deep_filter"
        folder.mkdir()
        (folder / "Cargo.toml").write_text("LOCAL MANIFEST FIXTURE ONLY", encoding="utf-8")
        (folder / "LICENSE").write_text("LOCAL LICENSE FIXTURE ONLY", encoding="utf-8")
        data["packages"].append({"id": "deep_filter", "name": "deep_filter", "version": "0.1.0",
                                 "source": None, "license": "MIT", "manifest_path": str(folder / "Cargo.toml")})
        data["resolve"]["nodes"][0]["deps"].append({"pkg": "deep_filter", "dep_kinds": [{"kind": None}]})
        data["resolve"]["nodes"].append({"id": "deep_filter", "deps": []})
        for name in ["LICENSE", "ThirdPartyNotices.txt"]:
            path = cache / "onnxruntime-win-x64-1.31.1/licenses" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("LOCAL ONNX LICENSE FIXTURE ONLY", encoding="utf-8")
        output = self.root / "inventory"
        with patch.object(PACKAGE, "run", side_effect=[str(sysroot), "LOCAL TOOLCHAIN FIXTURE"]):
            PACKAGE.collect_licenses(output, data, cache, True)
        inventory = json.loads((output / "inventory.json").read_text())
        entries = {item["name"]: item for item in inventory["packages"]}
        self.assertEqual(entries["deepfilternet3-model"]["assetSha256"],
                         PACKAGE.POLICY["modelFiles"]["DeepFilterNet3_onnx.tar.gz"])
        self.assertEqual(entries["silero-vad-model"]["assetSha256"], PACKAGE.POLICY["modelFiles"]["silero_vad.onnx"])
        self.assertFalse(inventory["complete"])
        self.assertFalse(inventory["legalApproval"])
        self.assertFalse(inventory["nativeClientReady"])


if __name__ == "__main__":
    unittest.main()
