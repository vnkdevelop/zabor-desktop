import argparse
import base64
import gzip
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile


SPEC = importlib.util.spec_from_file_location("closure", Path(__file__).with_name("reference-closure.py"))
CLOSURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLOSURE)


def prepare(retained):
    output = CLOSURE.OUTPUT
    names = ("capture-inputs.json", "capture-sources.json.gz", "reference.js.gz", "reference.css")
    if any((output / name).exists() for name in names):
        raise ValueError("Existing capture prerequisite artifacts preserved")
    cache = CLOSURE.ROOT / "rust/target-reference-archives"
    checked = CLOSURE.check(cache)
    manifest = json.loads((output / "closure.json").read_bytes())
    frozen_git = json.loads(gzip.decompress((output / "tailwind-sources.json.gz").read_bytes()))
    inputs, payload = [], {}
    for row in manifest["tailwind"]["sources"]:
        raw = CLOSURE.regular(retained / row["path"]).read_bytes()
        git_raw = base64.b64decode(frozen_git[row["path"]], validate=True)
        if raw == git_raw:
            checkout = "exact"
        elif raw.replace(b"\r\n", b"\n") == git_raw:
            checkout = "crlf"
        else:
            raise ValueError("Retained candidate differs from exact historical Git bytes: " + row["path"])
        inputs.append({"path": row["path"], "gitBlob": row["gitBlob"], "gitSha256": row["sha256"],
                       "size": len(raw), "sha256": CLOSURE.sha256(raw), "checkout": checkout})
        payload[row["path"]] = base64.b64encode(raw).decode()
    for name in ("src/renderer/src/locales/ru.json", "src/renderer/src/locales/en.json"):
        raw = CLOSURE.regular(retained / name).read_bytes()
        line = CLOSURE.git("ls-tree", CLOSURE.COMMIT, "--", name).decode().strip()
        blob = line.split(" ")[2].split("\t")[0]
        git_raw = CLOSURE.git("cat-file", "blob", blob)
        if raw.replace(b"\r\n", b"\n") != git_raw:
            raise ValueError("Retained capture catalog differs from historical Git bytes")
        inputs.append({"path": name, "gitBlob": blob, "gitSha256": CLOSURE.sha256(git_raw),
                       "size": len(raw), "sha256": CLOSURE.sha256(raw), "checkout": "exact" if raw == git_raw else "crlf"})
        payload[name] = base64.b64encode(raw).decode()
    bundle = gzip.compress(json.dumps(payload, sort_keys=True, separators=(",", ":")).encode(), mtime=0)
    base = Path.home() / "AppData/Local/Temp/opencode"
    with tempfile.TemporaryDirectory(prefix="native-reference-assets-", dir=base) as temporary:
        root = Path(temporary)
        for row in manifest["packages"]:
            raw = CLOSURE.regular(cache / row["archive"]).read_bytes()
            CLOSURE.archive_files(raw, row)
            with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
                for item in row["files"]:
                    target = root / row["path"] / item["path"]
                    target.parent.mkdir(parents=True, exist_ok=True)
                    content = archive.extractfile(row["archivePrefix"] + item["path"]).read()
                    if CLOSURE.sha256(content) != item["sha256"]:
                        raise ValueError("Materialized package bytes mismatch")
                    with target.open("xb") as stream:
                        stream.write(content)
        for name, encoded in payload.items():
            target = root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as stream:
                stream.write(base64.b64decode(encoded, validate=True))
        script = Path(__file__).with_name("compile-reference-assets.cjs")
        command = 'cmd.exe /d /v:off /s /c ""node" "' + str(script) + '" "' + str(root) + '""'
        environment = {name: os.environ[name] for name in ("PATH", "SystemRoot", "WINDIR", "ComSpec", "TEMP", "TMP",
                       "LOCALAPPDATA", "USERPROFILE") if name in os.environ}
        result = subprocess.run(command, cwd=root, env=environment, check=True, capture_output=True)
        compiler = json.loads(result.stdout)
        javascript = (root / "reference.js").read_bytes()
        css = (root / "reference.css").read_bytes()
        receipt = {"captureInputsVersion": 1, "sourceCommit": CLOSURE.COMMIT, "closureSha256": checked["closureSha256"],
                   "sources": inputs, "sourceBundle": "capture-sources.json.gz", "sourceBundleSha256": CLOSURE.sha256(bundle),
                   "tailwindContentPattern": manifest["tailwind"]["contentPattern"], "compiler": compiler,
                   "artifacts": [{"path": "reference.js.gz", "encoding": "gzip", "sha256": CLOSURE.sha256(javascript), "size": len(javascript)},
                                 {"path": "reference.css", "encoding": "exact", "sha256": CLOSURE.sha256(css), "size": len(css)}],
                    "requiredExternalArtifacts": manifest["requiredExternalArtifacts"], "referenceContractIntegrated": False,
                   "captureAccepted": False, "nativeClientReady": False}
    artifacts = {"capture-sources.json.gz": bundle, "reference.js.gz": gzip.compress(javascript, mtime=0),
                 "reference.css": css, "capture-inputs.json": (json.dumps(receipt, indent=2) + "\n").encode()}
    for name, raw in artifacts.items():
        with (output / name).open("xb") as stream:
            stream.write(raw)
    return {"sources": len(inputs), "compiler": compiler, "referenceContractIntegrated": False, "captureAccepted": False}


def check():
    root = CLOSURE.OUTPUT
    cache = CLOSURE.ROOT / "rust/target-reference-archives"
    closure = CLOSURE.check(cache)
    data = json.loads(CLOSURE.regular(root / "capture-inputs.json").read_bytes())
    raw = CLOSURE.regular(root / "capture-sources.json.gz").read_bytes()
    if (data["captureInputsVersion"] != 1 or data["sourceCommit"] != CLOSURE.COMMIT
            or data["sourceBundle"] != "capture-sources.json.gz" or CLOSURE.sha256(raw) != data["sourceBundleSha256"]
            or data["closureSha256"] != closure["closureSha256"]):
        raise ValueError("Capture prerequisite bundle provenance mismatch")
    sources = json.loads(gzip.decompress(raw))
    manifest = json.loads((root / "closure.json").read_bytes())
    expected = {row["path"] for row in manifest["tailwind"]["sources"]}
    expected.update(("src/renderer/src/locales/ru.json", "src/renderer/src/locales/en.json"))
    if (set(sources) != expected or len(data["sources"]) != len(expected)
            or {row["path"] for row in data["sources"]} != expected):
        raise ValueError("Capture source closure incomplete")
    for row in data["sources"]:
        raw = base64.b64decode(sources[row["path"]], validate=True)
        line = CLOSURE.git("ls-tree", CLOSURE.COMMIT, "--", row["path"]).decode().strip()
        git_raw = CLOSURE.git("cat-file", "blob", row["gitBlob"])
        normalized = raw.replace(b"\r\n", b"\n") if row["checkout"] == "crlf" else raw
        if (row["checkout"] not in ("crlf", "exact") or normalized != git_raw or len(raw) != row["size"]
                or CLOSURE.sha256(raw) != row["sha256"] or CLOSURE.sha256(git_raw) != row["gitSha256"]
                or line != "100644 blob " + row["gitBlob"] + "\t" + row["path"]):
            raise ValueError("Capture source/Git byte mismatch")
    if len(data["artifacts"]) != 2 or {row["path"] for row in data["artifacts"]} != {"reference.js.gz", "reference.css"}:
        raise ValueError("Compiled capture artifact closure incomplete")
    for row in data["artifacts"]:
        raw = CLOSURE.regular(root / row["path"]).read_bytes()
        if row["encoding"] != ("gzip" if row["path"] == "reference.js.gz" else "exact"):
            raise ValueError("Compiled capture artifact encoding mismatch")
        raw = gzip.decompress(raw) if row["encoding"] == "gzip" else raw
        if len(raw) != row["size"] or CLOSURE.sha256(raw) != row["sha256"]:
            raise ValueError("Compiled capture artifact hash/size mismatch")
    if any(data[key] is not False for key in ("referenceContractIntegrated", "captureAccepted", "nativeClientReady")):
        raise ValueError("Prerequisite bytes cannot grant capture acceptance")
    return {"sources": len(expected), "artifacts": len(data["artifacts"]), "captureAccepted": False,
            "captureInputsSha256": CLOSURE.sha256((root / "capture-inputs.json").read_bytes())}


def main():
    parser = argparse.ArgumentParser(description="Compile/check frozen reference CSS/JS only; no Electron app or npm installation")
    parser.add_argument("operation", choices=("prepare", "check"))
    parser.add_argument("--retained-root", type=Path)
    args = parser.parse_args()
    try:
        if args.operation == "prepare" and not args.retained_root:
            raise ValueError("Explicit retained source root required")
        result = prepare(args.retained_root) if args.operation == "prepare" else check()
        print(json.dumps(result, indent=2))
    except (ValueError, OSError, KeyError, TypeError, tarfile.TarError, subprocess.CalledProcessError) as error:
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
