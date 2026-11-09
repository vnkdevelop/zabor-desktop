import argparse
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import stat
import sys
import tarfile
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[3]
DEFAULT = ROOT / "licenses/native/supplements.json"


def load_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("Duplicate JSON field")
            result[key] = value
        return result

    return json.loads(raw, object_pairs_hook=pairs)


def regular(path):
    path = Path(path).absolute()
    for item in [*reversed(path.parents), path]:
        info = item.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("Supplement paths cannot contain redirected ancestors")
    if not path.is_file() or path.stat().st_nlink != 1:
        raise ValueError("Supplement must be a regular single-link file")
    return path


def sha256(raw):
    return hashlib.sha256(raw).hexdigest()


def verify(manifest=DEFAULT, cache_root=None):
    manifest = regular(manifest)
    data = load_json(manifest.read_bytes())
    if (data.get("supplementVersion") != 1 or data.get("legalApproval") is not False
            or not isinstance(data.get("texts"), list)):
        raise ValueError("Invalid supplement manifest")
    locked = tomllib.loads((ROOT / "rust/Cargo.lock").read_text(encoding="utf-8"))
    pins = {(item["name"], item["version"]): item.get("checksum") for item in locked["package"]}
    checked, seen = [], set()
    for item in data["texts"]:
        relative = item["file"]
        parts = relative.split("/") if isinstance(relative, str) else []
        if (not parts or any(not re.fullmatch(r"[A-Za-z0-9_.-]+", part)
                or part in (".", "..") or part.endswith(".") for part in parts)):
            raise ValueError("Unsafe supplement relative path")
        path = regular(manifest.parent / relative)
        source = re.fullmatch(r"https://raw\.githubusercontent\.com/([^/]+/[^/]+)/([0-9a-f]{40})/([^?#]+)",
                              item["source"])
        raw = path.read_bytes()
        if (not source or not 0 < len(raw) <= 1024 * 1024 or type(item["size"]) is not int
                or len(raw) != item["size"]
                or sha256(raw) != item["sha256"] or hashlib.sha1(b"blob " + str(len(raw)).encode()
                + b"\0" + raw).hexdigest() != item["gitBlob"]):
            raise ValueError("Supplement immutable byte provenance mismatch")
        raw.decode("utf-8", errors="strict")
        key = (item["package"], item["version"])
        identity = (*key, item["file"], item["source"])
        if identity in seen:
            raise ValueError("Duplicate supplement entry")
        seen.add(identity)
        if key[0] == "silero-vad-model":
            model = (ROOT / "rust/crates/zabor-native/assets/models/silero_vad.onnx").read_bytes()
            if (key[1] != "existing-local-model" or sha256(model) != item["assetSha256"]
                    or hashlib.sha1(b"blob " + str(len(model)).encode() + b"\0" + model).hexdigest()
                    != item["assetGitBlob"] or source.group(2) != item["assetRevision"]):
                raise ValueError("Silero model provenance mismatch")
        else:
            if pins.get(key) != item["registryChecksum"] or not pins.get(key):
                raise ValueError("Supplement package version/checksum differs from Cargo.lock")
            if cache_root:
                archive = regular(Path(cache_root) / (key[0] + "-" + key[1] + ".crate"))
                archive_raw = archive.read_bytes()
                if sha256(archive_raw) != pins[key]:
                    raise ValueError("Cached crate archive checksum mismatch")
                with tarfile.open(archive, "r:gz") as package:
                    entry = package.extractfile(key[0] + "-" + key[1] + "/.cargo_vcs_info.json")
                    if entry is None:
                        raise ValueError("Published crate VCS metadata missing")
                    vcs = load_json(entry.read(65537))
                if (vcs["git"]["sha1"] != source.group(2) or vcs != item["publishedVcs"]):
                    raise ValueError("Published crate revision differs from official supplement")
        checked.append(key)
    plan = load_json((ROOT / "licenses/native/upstream-plan.json").read_bytes())
    required = {(name, version) for name in plan["crates"] for package, version in pins if package == name}
    required.add(("silero-vad-model", "existing-local-model"))
    if set(checked) != required:
        raise ValueError("Supplement missing or unexpected pinned component")
    for name, paths in plan["crates"].items():
        collected = {item["source"].split("/", 6)[6] for item in data["texts"] if item["package"] == name}
        if collected != set(paths):
            raise ValueError("Supplement required upstream notice set differs")
    attributions = load_json((ROOT / "licenses/native/attributions.json").read_bytes())
    if (attributions["attributionVersion"] != 1 or len(attributions["attributions"]) != 4
            or {row["package"] for row in attributions["attributions"]}
            != {"const-field-offset", "const-field-offset-macro", "vtable", "vtable-macro"}):
        raise ValueError("Required upstream copyright attribution closure differs")
    for row in attributions["attributions"]:
        key = (row["package"], row["version"])
        header = base64.b64decode(row["headerBase64"], validate=True)
        entry = next(item for item in data["texts"] if (item["package"], item["version"]) == key)
        if (sha256(header) != row["headerSha256"] or row["byteRange"] != [0, len(header)]
                or row["registryChecksum"] != pins[key]
                or row["source"] != "https://raw.githubusercontent.com/slint-ui/slint/" + entry["publishedVcs"]["git"]["sha1"]
                + "/" + entry["publishedVcs"]["path_in_vcs"] + "/" + row["cratePath"]):
            raise ValueError("Upstream copyright attribution provenance mismatch")
        if cache_root:
            with tarfile.open(Path(cache_root) / (key[0] + "-" + key[1] + ".crate"), "r:gz") as package:
                raw = package.extractfile(key[0] + "-" + key[1] + "/" + row["cratePath"]).read(1024 * 1024 + 1)
            if (len(raw) != row["sourceSize"] or sha256(raw) != row["sourceSha256"] or not raw.startswith(header)
                    or hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest() != row["sourceGitBlob"]):
                raise ValueError("Published source copyright bytes differ from upstream")
    return {"texts": len(checked), "components": len(set(checked)), "manifestSha256": sha256(manifest.read_bytes()),
            "copyrightAttributions": len(attributions["attributions"]),
            "publishedCrateProvenance": "checked" if cache_root else "not-checked",
            "legalApproval": False, "nativeClientReady": False}


def main():
    parser = argparse.ArgumentParser(description="Offline exact upstream licence/version provenance check")
    parser.add_argument("--manifest", type=Path, default=DEFAULT)
    parser.add_argument("--cargo-cache", type=Path)
    parser.add_argument("--inventory-cache", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        result = verify(args.manifest, args.cargo_cache)
        if args.inventory_cache:
            spec = importlib.util.spec_from_file_location("native_package", Path(__file__).with_name("package.py"))
            package = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(package)
            package.check_cache(args.inventory_cache, True)
            temporary_parent = Path.home() / "AppData/Local/Temp/opencode"
            with tempfile.TemporaryDirectory(prefix="native-legal-inventory-", dir=temporary_parent) as temporary:
                destination = Path(temporary)
                count = package.collect_licenses(destination, package.metadata(True), args.inventory_cache, True, args.manifest)
                inventory = load_json((destination / "inventory.json").read_bytes())
                result.update({"inventoryComponents": count, "missingUpstreamTexts": inventory["missingUpstreamTexts"],
                               "textCollectionComplete": inventory["complete"],
                               "uniquePayloadTexts": len({text["path"] for item in inventory["packages"] for text in item["texts"]}),
                               "cargoLockSha256": inventory["cargoLockSha256"], "nativeCachePins": "checked"})
        rendered = json.dumps(result, indent=2) + "\n"
        if args.output:
            if not args.output.absolute().is_relative_to(ROOT):
                raise ValueError("Verification output must belong to this worktree")
            with args.output.open("x", encoding="utf-8", newline="\n") as stream:
                stream.write(rendered)
        print(rendered, end="")
    except (ValueError, OSError, KeyError, TypeError, tarfile.TarError) as error:
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
