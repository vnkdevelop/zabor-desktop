import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("supplements", Path(__file__).with_name("check_supplements.py"))
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class SupplementTests(unittest.TestCase):
    def setUp(self):
        self.owner = tempfile.TemporaryDirectory(prefix="native-supplement-test-", dir=Path.home() / "AppData/Local/Temp/opencode")
        self.folder = Path(self.owner.name)
        self.data = CHECK.load_json(CHECK.DEFAULT.read_bytes())

    def tearDown(self):
        self.owner.cleanup()

    def changed(self, transform):
        data = copy.deepcopy(self.data)
        transform(data)
        for row in data["texts"]:
            if isinstance(row.get("file"), str) and row["file"].startswith("texts/"):
                target = self.folder / row["file"]
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((CHECK.DEFAULT.parent / row["file"]).read_bytes())
        path = self.folder / "supplements.json"
        path.write_text(json.dumps(data), encoding="utf-8")
        return path

    def test_exact_collected_texts_do_not_grant_approval(self):
        result = CHECK.verify()
        self.assertEqual((result["components"], result["texts"]), (24, 42))
        self.assertFalse(result["legalApproval"])
        self.assertFalse(result["nativeClientReady"])

    def test_missing_component_is_not_complete(self):
        path = self.changed(lambda data: data.update(texts=[row for row in data["texts"] if row["package"] != "silero-vad-model"]))
        with self.assertRaisesRegex(ValueError, "missing or unexpected"):
            CHECK.verify(path)

    def test_missing_notice_is_not_complete(self):
        path = self.changed(lambda data: data["texts"].pop(2))
        with self.assertRaisesRegex(ValueError, "notice set"):
            CHECK.verify(path)

    def test_duplicate_and_unpinned_version_are_rejected(self):
        for transform, reason in [(lambda data: data["texts"].append(data["texts"][0]), "Duplicate"),
                                  (lambda data: data["texts"][0].update(version="0.0.0"), "Cargo.lock")]:
            path = self.changed(transform)
            with self.assertRaisesRegex(ValueError, reason):
                CHECK.verify(path)

    def test_changed_text_hash_blob_and_model_are_rejected(self):
        for transform, reason in [(lambda data: data["texts"][0].update(sha256="0" * 64), "byte provenance"),
                                  (lambda data: data["texts"][0].update(gitBlob="0" * 40), "byte provenance"),
                                  (lambda data: data["texts"][-1].update(assetSha256="0" * 64), "model provenance")]:
            path = self.changed(transform)
            with self.assertRaisesRegex(ValueError, reason):
                CHECK.verify(path)

    def test_mutable_url_and_approval_are_rejected(self):
        for transform, reason in [(lambda data: data["texts"][0].update(source=data["texts"][0]["source"].replace("a55d3e1a18bb9ef0e4bccc9083fb13c3e0ad8969", "main")), "byte provenance"),
                                  (lambda data: data.update(legalApproval=True), "Invalid supplement")]:
            path = self.changed(transform)
            with self.assertRaisesRegex(ValueError, reason):
                CHECK.verify(path)

    def test_redirected_path_and_duplicate_json_are_rejected(self):
        with patch.object(CHECK.Path, "lstat") as inspect:
            inspect.return_value.st_mode = 0o100644
            inspect.return_value.st_file_attributes = 0x400
            with self.assertRaisesRegex(ValueError, "redirected ancestors"):
                CHECK.regular(self.folder / "unread-file")
        with self.assertRaisesRegex(ValueError, "Duplicate JSON"):
            CHECK.load_json('{"texts":[],"texts":[]}')

    def test_original_copyright_header_digest_is_required(self):
        loader = CHECK.load_json

        def corrupt_attribution(raw):
            data = loader(raw)
            if isinstance(data, dict) and "attributionVersion" in data:
                data["attributions"][0]["headerSha256"] = "0" * 64
            return data

        with patch.object(CHECK, "load_json", side_effect=corrupt_attribution):
            with self.assertRaisesRegex(ValueError, "copyright attribution provenance"):
                CHECK.verify()


if __name__ == "__main__":
    unittest.main()
