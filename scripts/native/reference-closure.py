import argparse
import base64
import concurrent.futures
import gzip
import hashlib
import io
import importlib.util
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tarfile
import urllib.request


ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "tests/native-delivery"
COMMIT = "c8a622f727a60a269752013d1e222b9864c50d49"
LOCK_BLOB = "8c1cd9279bf5a055cb80a9508e26866e0a787f70"
LOCK_HASH = "a199ccaa069437b9580c67fbbed0481a5e2fa58dde1c4e577398c9950c186b87"
ROOTS = ("typescript", "@echogarden/fvad-wasm", "onnxruntime-web", "@timephy/rnnoise-wasm",
         "deepfilter-standalone", "electron", "esbuild", "postcss", "tailwindcss", "react",
         "react-dom", "i18next", "react-i18next", "@phosphor-icons/react")


def sha256(raw):
    return hashlib.sha256(raw).hexdigest()


def git(*parts):
    if any(re.search(r'["%\r\n]', part) for part in parts):
        raise ValueError("Unsafe Git arguments")
    command = " ".join('"' + part + '"' for part in ("git", *parts))
    return subprocess.run('cmd.exe /d /v:off /s /c "' + command + '"', cwd=ROOT,
                          check=True, capture_output=True, timeout=120).stdout


def lock_bytes():
    listing = git("ls-tree", COMMIT, "--", "package-lock.json").decode().strip()
    if listing != "100644 blob " + LOCK_BLOB + "\tpackage-lock.json":
        raise ValueError("Historical lock Git provenance mismatch")
    raw = git("cat-file", "blob", LOCK_BLOB)
    if sha256(raw) != LOCK_HASH:
        raise ValueError("Historical lock hash mismatch")
    return raw


def compatible(item):
    def accepts(values, target):
        return ("!" + target not in values and (not values or target in values
                or all(value.startswith("!") for value in values)))

    return accepts(item.get("os", []), "win32") and accepts(item.get("cpu", []), "x64")


def resolve(packages, origin, name):
    directory = origin
    while directory:
        if directory.split("/")[-1] != "node_modules":
            candidate = directory + "/node_modules/" + name
            if candidate in packages:
                return candidate
        directory = directory.rsplit("/", 1)[0] if "/" in directory else ""
    candidate = "node_modules/" + name
    return candidate if candidate in packages else None


def closure(lock):
    packages = lock["packages"]
    pending = ["node_modules/" + name for name in ROOTS]
    selected, edges, excluded = set(), [], []
    while pending:
        key = pending.pop()
        if key in selected:
            continue
        item = packages[key]
        if not compatible(item) or item.get("link"):
            raise ValueError("Required package unavailable on Windows x64: " + key)
        selected.add(key)
        optional = item.get("optionalDependencies", {})
        for kind, declarations in [("normal", item.get("dependencies", {})), ("optional", optional),
                                   ("peer", item.get("peerDependencies", {}))]:
            for name, request in sorted(declarations.items()):
                if kind == "normal" and name in optional:
                    continue
                target = resolve(packages, key, name)
                may_skip = kind == "optional" or (kind == "peer" and item.get("peerDependenciesMeta", {}).get(name, {}).get("optional"))
                if not target or not compatible(packages[target]):
                    if not may_skip:
                        raise ValueError("Missing required lock dependency: " + key + ": " + name)
                    excluded.append({"from": key, "name": name, "kind": kind, "request": request,
                                     "reason": "absent-optional-peer" if not target else "not-windows-x64"})
                    continue
                edges.append({"from": key, "to": target, "name": name, "kind": kind, "request": request})
                pending.append(target)
    rows = []
    for key in sorted(selected):
        item = packages[key]
        if (not re.fullmatch(r"https://registry\.npmjs\.org/[^?#]+\.tgz", item.get("resolved", ""))
                or not re.fullmatch(r"sha512-[A-Za-z0-9+/]+={0,2}", item.get("integrity", ""))):
            raise ValueError("Package lacks pinned official archive: " + key)
        rows.append({"path": key, "version": item["version"], "source": item["resolved"], "integrity": item["integrity"],
                     "archive": base64.b64decode(item["integrity"][7:], validate=True).hex() + ".tgz"})
    return {"packages": rows, "edges": sorted(edges, key=lambda row: (row["from"], row["kind"], row["name"])),
            "excluded": sorted(excluded, key=lambda row: (row["from"], row["kind"], row["name"]))}


def archive_files(raw, row):
    expected = base64.b64decode(row["integrity"][7:], validate=True)
    if len(expected) != 64 or hashlib.sha512(raw).digest() != expected:
        raise ValueError("Package archive SRI mismatch: " + row["path"])
    entries, total, directories = [], 0, set()
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        members = archive.getmembers()
        if len(members) > 40000:
            raise ValueError("Oversized package entry set")
        prefixes = {item.name.split("/")[0] for item in members}
        if len(prefixes) != 1 or not re.fullmatch(r"[A-Za-z0-9_. -]+", next(iter(prefixes))):
            raise ValueError("Ambiguous package archive root")
        prefix = next(iter(prefixes)) + "/"
        for item in members:
            name = item.name.removeprefix(prefix)
            if item.isdir():
                directories.add(name)
                continue
            parts = name.split("/")
            if (not item.name.startswith(prefix) or not item.isfile() or any(not part or part in (".", "..")
                    or re.search(r'[\\:\x00-\x1f]', part) or part.endswith((".", " "))
                    or re.match(r"^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)", part, re.I) for part in parts)):
                raise ValueError("Unsafe package archive path/type: " + row["path"] + ": " + item.name)
            total += item.size
            if not 0 <= item.size <= 256 * 1024 * 1024 or total > 1024 * 1024 * 1024:
                raise ValueError("Oversized package contents")
            content = archive.extractfile(item).read()
            entries.append({"path": name, "size": len(content), "sha256": sha256(content)})
    if len({item["path"].lower() for item in entries}) != len(entries):
        raise ValueError("Case-colliding package paths")
    paths = {item["path"].lower() for item in entries}
    if any("/".join(PurePosixPath(name).parts[:index]).lower() in paths
           for name in paths for index in range(1, len(PurePosixPath(name).parts))):
        raise ValueError("Package file/directory collision")
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        manifest = json.load(archive.extractfile(prefix + "package.json"))
    if (manifest.get("version") != row["version"]
            or manifest.get("name") != row["path"].split("node_modules/")[-1]):
        raise ValueError("Package manifest identity/version mismatch")
    return {"archiveSha256": sha256(raw), "archiveSize": len(raw), "archivePrefix": prefix,
            "files": sorted(entries, key=lambda item: item["path"])}


def regular(path):
    path = path.absolute()
    for item in [*reversed(path.parents), path]:
        info = item.lstat()
        if item.is_symlink() or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("Reference closure path is redirected")
    if path.is_file() and path.stat().st_nlink != 1:
        raise ValueError("Reference closure hard link rejected")
    return path


def collect_candidates():
    listing = git("ls-tree", "-r", "-z", COMMIT, "--", "src/").split(b"\0")
    rows, bundle = [], {}
    for line in sorted(filter(None, listing)):
        header, name = line.split(b"\t", 1)
        mode, kind, blob = header.decode().split(" ")
        name = name.decode()
        if PurePosixPath(name).suffix not in (".js", ".jsx", ".ts", ".tsx", ".html"):
            continue
        if mode != "100644" or kind != "blob":
            raise ValueError("Candidate closure must use regular Git sources")
        raw = git("cat-file", "blob", blob)
        raw.decode("utf-8", errors="strict")
        rows.append({"path": name, "gitBlob": blob, "size": len(raw), "sha256": sha256(raw)})
        bundle[name] = base64.b64encode(raw).decode()
    for name in ("src/renderer/src/index.css", "tailwind.config.js"):
        line = git("ls-tree", COMMIT, "--", name).decode().strip()
        blob = line.split(" ")[2].split("\t")[0]
        raw = git("cat-file", "blob", blob)
        rows.append({"path": name, "gitBlob": blob, "size": len(raw), "sha256": sha256(raw)})
        bundle[name] = base64.b64encode(raw).decode()
    raw = gzip.compress(json.dumps(bundle, sort_keys=True, separators=(",", ":")).encode(), mtime=0)
    return rows, raw


def prepare(cache, fetch_official):
    expected_cache = ROOT / "rust/target-reference-archives"
    if cache.absolute() != expected_cache:
        raise ValueError("Only the isolated worktree reference archive cache is writable")
    if (OUTPUT / "closure.json").exists() or (OUTPUT / "tailwind-sources.json.gz").exists():
        raise ValueError("Existing closure artifacts preserved")
    regular(cache.parent)
    cache.mkdir(exist_ok=True)
    regular(cache)
    data = closure(json.loads(lock_bytes()))

    def admit(row):
        path = cache / row["archive"]
        if path.exists():
            raw = regular(path).read_bytes()
        else:
            if not fetch_official:
                raise ValueError("Missing exact reference archive: " + row["path"])
            request = urllib.request.Request(row["source"], headers={"User-Agent": "ZABOR-reference-provenance"})
            with urllib.request.urlopen(request, timeout=60) as response:
                if response.url != row["source"]:
                    raise ValueError("Unexpected npm archive redirect")
                raw = response.read(256 * 1024 * 1024 + 1)
            if len(raw) > 256 * 1024 * 1024:
                raise ValueError("Oversized npm archive")
        files = archive_files(raw, row)
        if not path.exists():
            with path.open("xb") as stream:
                stream.write(raw)
        return {**row, **files}

    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as executor:
        rows = list(executor.map(admit, data["packages"]))
    candidates, bundle = collect_candidates()
    result = {"closureVersion": 1, "sourceCommit": COMMIT, "packageLockGitBlob": LOCK_BLOB,
              "packageLockSha256": LOCK_HASH, "target": "win32-x64", "roots": list(ROOTS),
              "scope": "one-off test/reference inputs only; no native shipping dependency",
              "packages": rows, "edges": data["edges"], "excluded": data["excluded"],
              "tailwind": {"contentPattern": "./src/**/*.{js,jsx,ts,tsx,html}", "sourceEncoding": "exact Git UTF-8/LF",
                           "sources": candidates, "bundle": "tailwind-sources.json.gz", "bundleSha256": sha256(bundle)},
              "requiredExternalArtifacts": [{"package": "electron", "version": "44.4.3",
                                       "source": "https://github.com/electron/electron/releases/download/v44.4.3/electron-v44.4.3-win32-x64.zip",
                                       "reason": "outside npm archive; exact runtime/SHASUMS are supplied by electron-reference.json"}],
              "referenceContractIntegrated": False, "captureAccepted": False, "nativeClientReady": False}
    OUTPUT.mkdir(parents=True, exist_ok=True)
    with (OUTPUT / "tailwind-sources.json.gz").open("xb") as stream:
        stream.write(bundle)
    with (OUTPUT / "closure.json").open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(result, stream, indent=2)
        stream.write("\n")
    return {"packages": len(rows), "files": sum(len(row["files"]) for row in rows),
            "tailwindSources": len(candidates), "requiredExternalArtifacts": result["requiredExternalArtifacts"],
            "referenceContractIntegrated": False, "captureAccepted": False}


def check(cache):
    manifest = json.loads(regular(OUTPUT / "closure.json").read_bytes())
    expected = closure(json.loads(lock_bytes()))
    if (manifest["closureVersion"] != 1 or manifest["sourceCommit"] != COMMIT or manifest["packageLockGitBlob"] != LOCK_BLOB
            or manifest["packageLockSha256"] != LOCK_HASH or manifest["roots"] != list(ROOTS)
            or manifest["target"] != "win32-x64" or manifest["edges"] != expected["edges"]
            or manifest["excluded"] != expected["excluded"] or len(manifest["packages"]) != len(expected["packages"])):
        raise ValueError("Reference dependency closure differs from pinned historical lock")
    for row, pin in zip(manifest["packages"], expected["packages"]):
        if any(row[key] != value for key, value in pin.items()):
            raise ValueError("Reference archive pin differs from lock")
        actual = archive_files(regular(cache / row["archive"]).read_bytes(), pin)
        if any(row[key] != value for key, value in actual.items()):
            raise ValueError("Reference archive file inventory mismatch")
    candidates, bundle = collect_candidates()
    tailwind = manifest["tailwind"]
    if (tailwind["bundle"] != "tailwind-sources.json.gz" or candidates != tailwind["sources"]
            or sha256(bundle) != tailwind["bundleSha256"]
            or regular(OUTPUT / tailwind["bundle"]).read_bytes() != bundle):
        raise ValueError("Whole-source Tailwind closure mismatch")
    if any(manifest[key] is not False for key in ("referenceContractIntegrated", "captureAccepted", "nativeClientReady")):
        raise ValueError("Prerequisite collection cannot grant reference/capture acceptance")
    spec = importlib.util.spec_from_file_location("electron_reference", Path(__file__).with_name("electron-reference.py"))
    electron = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(electron)
    runtime = electron.check()
    return {"packages": len(manifest["packages"]), "files": sum(len(row["files"]) for row in manifest["packages"]),
            "tailwindSources": len(candidates), "closureSha256": sha256((OUTPUT / "closure.json").read_bytes()),
            "externalRuntime": runtime, "unresolvedArtifacts": [], "referenceContractIntegrated": False, "captureAccepted": False}


def main():
    parser = argparse.ArgumentParser(description="Pinned test-only dependency/candidate closure; no npm install or app startup")
    parser.add_argument("operation", choices=("prepare", "check"))
    parser.add_argument("--cache-root", type=Path, default=ROOT / "rust/target-reference-archives")
    parser.add_argument("--fetch-official", action="store_true")
    args = parser.parse_args()
    try:
        if args.operation == "check" and args.fetch_official:
            raise ValueError("Check is strictly offline")
        result = prepare(args.cache_root, args.fetch_official) if args.operation == "prepare" else check(args.cache_root)
        print(json.dumps(result, indent=2))
    except (ValueError, OSError, KeyError, TypeError, tarfile.TarError, subprocess.CalledProcessError) as error:
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
