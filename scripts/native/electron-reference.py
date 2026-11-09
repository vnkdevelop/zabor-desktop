import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import tempfile
import urllib.request
import zipfile


SPEC = importlib.util.spec_from_file_location("closure", Path(__file__).with_name("reference-closure.py"))
CLOSURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLOSURE)
NAME = "electron-v44.4.3-win32-x64.zip"
VERSION = "44.4.3"
HASH = "790a355b684d5c7cc8dc3cdd8c4cca7c4b2d054685427c7554a956879a82e70b"
BASE = "https://github.com/electron/electron/releases/download/v" + VERSION + "/"


def inspect(path):
    if not 0 < path.stat().st_size <= 1024 * 1024 * 1024:
        raise ValueError("Electron reference archive size exceeds bound")
    with path.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != HASH:
            raise ValueError("Pinned official Electron reference archive hash mismatch")
    rows = []
    with zipfile.ZipFile(path) as archive:
        members = archive.infolist()
        if len(members) > 10000 or sum(item.file_size for item in members) > 2 * 1024 * 1024 * 1024:
            raise ValueError("Oversized Electron reference contents")
        for item in members:
            if item.is_dir():
                continue
            if (item.flag_bits & 1 or (item.external_attr >> 16) & 0o170000 not in (0, 0o100000)
                    or any(not part or part in (".", "..") or re.search(r'[\\:\x00-\x1f]', part)
                    or part.endswith((".", " ")) for part in item.filename.split("/"))):
                raise ValueError("Unsafe Electron reference archive path/type")
            with archive.open(item) as stream:
                checksum = hashlib.file_digest(stream, "sha256").hexdigest()
            rows.append({"path": item.filename, "size": item.file_size, "sha256": checksum})
        if len({row["path"].lower() for row in rows}) != len(rows):
            raise ValueError("Electron reference path collision")
        names = {row["path"] for row in rows}
        if not {"electron.exe", "version", "LICENSE", "LICENSES.chromium.html"}.issubset(names):
            raise ValueError("Incomplete Electron runtime/notice closure")
        if archive.read("version").decode().strip() != VERSION:
            raise ValueError("Electron runtime version differs from historical lock")
        spec = importlib.util.spec_from_file_location("package", Path(__file__).parent / "packaging/package.py")
        package = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(package)
        package.inspect_pe(archive.read("electron.exe"))
    return sorted(rows, key=lambda row: row["path"])


def prepare():
    output = CLOSURE.OUTPUT
    names = ("electron-reference.json", "electron-SHASUMS256.txt")
    if any((output / name).exists() for name in names):
        raise ValueError("Existing Electron reference provenance preserved")
    cache = CLOSURE.regular(CLOSURE.ROOT / "rust/target-reference-archives")
    request = urllib.request.Request(BASE + "SHASUMS256.txt", headers={"User-Agent": "ZABOR-reference-provenance"})
    with urllib.request.urlopen(request, timeout=60) as response:
        shasums = response.read(1024 * 1024 + 1)
    if len(shasums) > 1024 * 1024 or shasums.decode().splitlines().count(HASH + " *" + NAME) != 1:
        raise ValueError("Official Electron SHASUMS differs from pinned reference digest")
    path = cache / NAME
    if not path.exists():
        with tempfile.TemporaryDirectory(prefix="electron-reference-download-", dir=cache) as temporary:
            candidate = Path(temporary) / NAME
            request = urllib.request.Request(BASE + NAME, headers={"User-Agent": "ZABOR-reference-provenance"})
            total = 0
            with urllib.request.urlopen(request, timeout=60) as response, candidate.open("xb") as stream:
                while chunk := response.read(1024 * 1024):
                    total += len(chunk)
                    if total > 1024 * 1024 * 1024:
                        raise ValueError("Electron download exceeds bound")
                    stream.write(chunk)
            inspect(candidate)
            if path.exists():
                raise ValueError("Racing reference archive preserved")
            candidate.rename(path)
    rows = inspect(CLOSURE.regular(path))
    receipt = {"electronReferenceVersion": 1, "version": VERSION, "target": "win32-x64", "source": BASE + NAME,
               "sha256": HASH, "archive": NAME, "size": path.stat().st_size,
               "shasumsSource": BASE + "SHASUMS256.txt", "shasumsSha256": CLOSURE.sha256(shasums),
               "files": rows, "role": "one-off original Chromium test reference only; forbidden native package payload",
               "publisherTrust": "not-claimed", "referenceContractIntegrated": False, "captureAccepted": False}
    with (output / "electron-SHASUMS256.txt").open("xb") as stream:
        stream.write(shasums)
    with (output / "electron-reference.json").open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(receipt, stream, indent=2)
        stream.write("\n")
    return {"runtimeVersion": VERSION, "archiveSha256": HASH, "files": len(rows),
            "archiveSize": path.stat().st_size, "referenceContractIntegrated": False, "captureAccepted": False}


def check():
    output = CLOSURE.OUTPUT
    data = json.loads(CLOSURE.regular(output / "electron-reference.json").read_bytes())
    path = CLOSURE.regular(CLOSURE.ROOT / "rust/target-reference-archives" / NAME)
    shasums = CLOSURE.regular(output / "electron-SHASUMS256.txt").read_bytes()
    if (data["electronReferenceVersion"] != 1 or data["version"] != VERSION or data["target"] != "win32-x64"
            or data["source"] != BASE + NAME or data["sha256"] != HASH or data["archive"] != NAME
            or data["size"] != path.stat().st_size or data["shasumsSource"] != BASE + "SHASUMS256.txt"
            or data["shasumsSha256"] != CLOSURE.sha256(shasums) or shasums.decode().splitlines().count(HASH + " *" + NAME) != 1
            or data["files"] != inspect(path)):
        raise ValueError("Electron runtime reference provenance mismatch")
    if data["referenceContractIntegrated"] is not False or data["captureAccepted"] is not False:
        raise ValueError("Electron archive integrity cannot grant capture acceptance")
    return {"runtimeVersion": VERSION, "archiveSha256": HASH, "files": len(data["files"]), "captureAccepted": False}


def main():
    parser = argparse.ArgumentParser(description="Pinned official Electron test archive only; no extraction/install/app startup")
    parser.add_argument("operation", choices=("prepare", "check"))
    parser.add_argument("--fetch-official", action="store_true")
    args = parser.parse_args()
    try:
        if args.operation == "prepare" and not args.fetch_official:
            raise ValueError("Explicit official reference acquisition required")
        if args.operation == "check" and args.fetch_official:
            raise ValueError("Reference check is offline")
        print(json.dumps(prepare() if args.operation == "prepare" else check(), indent=2))
    except (ValueError, OSError, KeyError, TypeError, zipfile.BadZipFile) as error:
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
