import base64
import copy
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("closure", Path(__file__).with_name("reference-closure.py"))
CLOSURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLOSURE)


def item(version="1.0.0", **extra):
    return {"version": version, "resolved": "https://registry.npmjs.org/fixture/-/fixture-1.0.0.tgz",
            "integrity": "sha512-" + base64.b64encode(bytes(64)).decode(), **extra}


def archive(files, prefix="package/"):
    target = io.BytesIO()
    with tarfile.open(fileobj=target, mode="w:gz") as output:
        for name, raw, kind in files:
            entry = tarfile.TarInfo(prefix + name)
            entry.type = kind
            entry.size = len(raw) if kind == tarfile.REGTYPE else 0
            output.addfile(entry, io.BytesIO(raw) if kind == tarfile.REGTYPE else None)
    raw = target.getvalue()
    return raw, {"path": "node_modules/fixture", "version": "1.0.0",
                 "integrity": "sha512-" + base64.b64encode(hashlib.sha512(raw).digest()).decode()}


class ReferenceClosureTests(unittest.TestCase):
    def setUp(self):
        self.lock = {"packages": {"node_modules/fixture": item(dependencies={"child": "^1"}, peerDependencies={"peer": "^1"}),
                                  "node_modules/child": item(), "node_modules/peer": item()}}
        self.roots = patch.object(CLOSURE, "ROOTS", ("fixture",))
        self.roots.start()
        self.addCleanup(self.roots.stop)

    def test_dependency_peer_and_nested_resolution_are_deterministic(self):
        self.lock["packages"]["node_modules/fixture/node_modules/child"] = item("1.1.0")
        expected = CLOSURE.closure(self.lock)
        self.assertEqual(expected, CLOSURE.closure(copy.deepcopy(self.lock)))
        self.assertEqual(len(expected["packages"]), 3)
        self.assertEqual(next(row for row in expected["edges"] if row["name"] == "child")["to"],
                         "node_modules/fixture/node_modules/child")

    def test_platform_optional_edges_are_explicit_not_required(self):
        self.lock["packages"]["node_modules/fixture"]["optionalDependencies"] = {"linux-binary": "1.0.0"}
        self.lock["packages"]["node_modules/linux-binary"] = item(os=["linux"], cpu=["x64"])
        result = CLOSURE.closure(self.lock)
        self.assertEqual(result["excluded"][0]["reason"], "not-windows-x64")
        self.assertEqual(len(result["packages"]), 3)

    def test_missing_required_or_unpinned_archive_fails_closed(self):
        del self.lock["packages"]["node_modules/peer"]
        with self.assertRaisesRegex(ValueError, "Missing required"):
            CLOSURE.closure(self.lock)
        self.lock["packages"]["node_modules/peer"] = item(resolved="https://fixture.invalid/unpinned.tgz")
        with self.assertRaisesRegex(ValueError, "pinned official"):
            CLOSURE.closure(self.lock)

    def test_negative_platform_selectors_are_enforced(self):
        self.assertFalse(CLOSURE.compatible(item(os=["!win32"])))
        self.assertFalse(CLOSURE.compatible(item(cpu=["arm64"])))
        self.assertTrue(CLOSURE.compatible(item(os=["!linux"], cpu=["x64"])))

    def fixture(self, extra=(), prefix="package/"):
        return archive([("package.json", b'{"name":"fixture","version":"1.0.0"}', tarfile.REGTYPE),
                        ("dist/main.js", b"module.exports = 1;", tarfile.REGTYPE), *extra], prefix)

    def test_archive_manifest_sri_and_all_relative_files_are_pinned(self):
        raw, row = self.fixture(prefix="fixture v1/")
        result = CLOSURE.archive_files(raw, row)
        self.assertEqual(result["archivePrefix"], "fixture v1/")
        self.assertEqual([item["path"] for item in result["files"]], ["dist/main.js", "package.json"])
        self.assertEqual(result["archiveSha256"], hashlib.sha256(raw).hexdigest())

    def test_sri_or_manifest_version_corruption_is_rejected(self):
        raw, row = self.fixture()
        with self.assertRaisesRegex(ValueError, "SRI"):
            CLOSURE.archive_files(raw + b"tampered", row)
        row["version"] = "9.0.0"
        with self.assertRaisesRegex(ValueError, "identity/version"):
            CLOSURE.archive_files(raw, row)

    def test_alias_traversal_and_link_members_are_rejected(self):
        for name, kind in [("../escape", tarfile.REGTYPE), ("A:stream", tarfile.REGTYPE),
                           ("NUL.txt", tarfile.REGTYPE), ("link", tarfile.SYMTYPE), ("hard", tarfile.LNKTYPE)]:
            raw, row = self.fixture([(name, b"fixture", kind)])
            with self.assertRaisesRegex(ValueError, "Unsafe"):
                CLOSURE.archive_files(raw, row)

    def test_duplicate_case_and_file_parent_collisions_are_rejected(self):
        for extra, reason in [([("DIST/main.js", b"duplicate", tarfile.REGTYPE)], "Case-colliding"),
                              ([("dist", b"file-parent", tarfile.REGTYPE)], "file/directory")]:
            raw, row = self.fixture(extra)
            with self.assertRaisesRegex(ValueError, reason):
                CLOSURE.archive_files(raw, row)


if __name__ == "__main__":
    unittest.main()
