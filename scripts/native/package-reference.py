import argparse
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile


SPEC = importlib.util.spec_from_file_location("closure", Path(__file__).with_name("reference-closure.py"))
CLOSURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLOSURE)
PIN = "8da37a64dd7cce433c842f9d8ee0e6cd9a6e69cae325b2b64b210d2e7531c4a7"
DESTINATION = CLOSURE.ROOT / "rust/target/native-reference-packages"


def inventory():
    raw = CLOSURE.regular(CLOSURE.OUTPUT / "closure.json").read_bytes()
    if CLOSURE.sha256(raw) != PIN:
        raise ValueError("Reference package inventory pin mismatch")
    manifest = json.loads(raw)
    expected = CLOSURE.closure(json.loads(CLOSURE.lock_bytes()))
    if (manifest["sourceCommit"] != CLOSURE.COMMIT or manifest["target"] != "win32-x64"
            or manifest["edges"] != expected["edges"] or manifest["excluded"] != expected["excluded"]
            or len(manifest["packages"]) != len(expected["packages"])):
        raise ValueError("Reference package dependency closure mismatch")
    for row, pin in zip(manifest["packages"], expected["packages"]):
        if any(row[key] != value for key, value in pin.items()):
            raise ValueError("Reference package differs from historical lock")
    return manifest


def check(root):
    manifest = inventory()
    root = CLOSURE.regular(root)
    if not root.is_dir():
        raise ValueError("Reference package directory missing")
    expected = set()
    directories = {root}
    for row in manifest["packages"]:
        for item in row["files"]:
            relative = row["path"] + "/" + item["path"]
            target = CLOSURE.regular(root / relative)
            raw = target.read_bytes()
            if len(raw) != item["size"] or CLOSURE.sha256(raw) != item["sha256"]:
                raise ValueError("Reference package file mismatch: " + relative)
            expected.add(target)
            directories.update(parent for parent in target.parents if parent.is_relative_to(root))
    for target in root.rglob("*"):
        CLOSURE.regular(target)
        if target not in (directories if target.is_dir() else expected):
            raise ValueError("Unexpected reference package path: " + str(target.relative_to(root)))
    return {"root": str(root.resolve()), "closureSha256": PIN, "packages": len(manifest["packages"]),
            "files": len(expected), "nativeClientReady": False, "captureAccepted": False}


def prepare():
    if DESTINATION.exists():
        raise ValueError("Existing package reference preserved; use check")
    manifest = inventory()
    cache = CLOSURE.ROOT / "rust/target-reference-archives"
    DESTINATION.parent.mkdir(parents=True, exist_ok=True)
    CLOSURE.regular(DESTINATION.parent)
    with tempfile.TemporaryDirectory(prefix=".reference-packages-", dir=DESTINATION.parent) as temporary:
        root = Path(temporary)
        materialized = root / "packages"
        materialized.mkdir()
        for row in manifest["packages"]:
            raw = CLOSURE.regular(cache / row["archive"]).read_bytes()
            actual = CLOSURE.archive_files(raw, row)
            if any(row[key] != value for key, value in actual.items()):
                raise ValueError("Reference archive inventory mismatch")
            with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
                for item in row["files"]:
                    target = materialized / row["path"] / item["path"]
                    target.parent.mkdir(parents=True, exist_ok=True)
                    content = archive.extractfile(row["archivePrefix"] + item["path"]).read()
                    with target.open("xb") as stream:
                        stream.write(content)
        check(materialized)
        materialized.rename(DESTINATION)
    return check(DESTINATION)


def main():
    parser = argparse.ArgumentParser(description="Offline pinned test-only npm package reference; no npm install or lifecycle scripts")
    parser.add_argument("operation", choices=("prepare", "check"))
    parser.add_argument("--root", type=Path, default=DESTINATION)
    args = parser.parse_args()
    try:
        if args.operation == "prepare" and args.root != DESTINATION:
            raise ValueError("Preparation uses only the isolated worktree reference cache")
        print(json.dumps(prepare() if args.operation == "prepare" else check(args.root), indent=2))
    except (ValueError, OSError, KeyError, TypeError, tarfile.TarError) as error:
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
