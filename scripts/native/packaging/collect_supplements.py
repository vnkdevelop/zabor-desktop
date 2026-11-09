import concurrent.futures
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys
import tomllib
import urllib.request


ROOT = Path(__file__).resolve().parents[3]


def fetch(url, limit=1024 * 1024):
    request = urllib.request.Request(url, headers={"User-Agent": "ZABOR-license-provenance"})
    with urllib.request.urlopen(request, timeout=60) as response:
        if response.url != url:
            raise ValueError("Unexpected official-source redirect")
        raw = response.read(limit + 1)
    if not 0 < len(raw) <= limit:
        raise ValueError("Official licence response exceeds bound")
    return raw


def main():
    if sys.argv[1:] == ["--fetch-attributions"]:
        collect_attributions()
        return
    if sys.argv[1:] != ["--fetch-official"]:
        raise ValueError("Explicit --fetch-official required; no dependency download or installation")
    folder = ROOT / "licenses/native"
    output = folder / "supplements.json"
    if output.exists() or (folder / "texts").exists():
        raise ValueError("Refusing to overwrite collected upstream texts")
    plan = json.loads((folder / "upstream-plan.json").read_text())
    spec = importlib.util.spec_from_file_location("native_package", Path(__file__).with_name("package.py"))
    package = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(package)
    metadata = package.metadata(True)
    locked = tomllib.loads((ROOT / "rust/Cargo.lock").read_text())
    pins = {(item["name"], item["version"]): item.get("checksum") for item in locked["package"]}
    entries, trees = [], {}
    for name, files in sorted(plan["crates"].items()):
        matches = [item for item in metadata["packages"] if item["name"] == name]
        if len(matches) != 1:
            raise ValueError("Ambiguous pinned package: " + name)
        item = matches[0]
        vcs = json.loads((Path(item["manifest_path"]).parent / ".cargo_vcs_info.json").read_text())
        repo = re.fullmatch(r"https://github\.com/([^/]+/[^/]+)/?", item["repository"])
        revision = vcs["git"]["sha1"]
        if not repo or not re.fullmatch(r"[0-9a-f]{40}", revision):
            raise ValueError("Immutable official package revision unavailable")
        repository = plan["officialRepositoryMoves"].get(repo.group(1), repo.group(1))
        tree_key = (repository, revision)
        if tree_key not in trees:
            trees[tree_key] = json.loads(fetch("https://api.github.com/repos/" + repository
                                               + "/git/trees/" + revision + "?recursive=1", 8 * 1024 * 1024))
        tree = trees[tree_key]
        if tree.get("truncated"):
            raise ValueError("Incomplete official source tree")
        blobs = {entry["path"]: entry for entry in tree["tree"] if entry["type"] == "blob"}
        for path in files:
            blob = blobs[path]
            entries.append({"package": name, "version": item["version"], "licenseExpression": item["license"],
                            "registryChecksum": pins[(name, item["version"])], "publishedVcs": vcs,
                            "source": "https://raw.githubusercontent.com/" + repository + "/" + revision + "/" + path,
                            "gitBlob": blob["sha"], "size": blob["size"],
                            "file": "texts/" + repo.group(1).replace("/", "_") + "/" + revision + "/" + path})
    silero = plan["silero"]
    tree = json.loads(fetch("https://api.github.com/repos/" + silero["repository"]
                            + "/git/trees/" + silero["revision"] + "?recursive=1"))
    blobs = {entry["path"]: entry for entry in tree["tree"] if entry["type"] == "blob"}
    if blobs[silero["modelPath"]]["sha"] != silero["modelGitBlob"]:
        raise ValueError("Official Silero tree does not contain exact retained model")
    blob = blobs[silero["licensePath"]]
    entries.append({"package": "silero-vad-model", "version": "existing-local-model", "licenseExpression": "MIT",
                    "source": "https://raw.githubusercontent.com/" + silero["repository"] + "/"
                    + silero["revision"] + "/" + silero["licensePath"], "gitBlob": blob["sha"], "size": blob["size"],
                    "file": "texts/snakers4_silero-vad/" + silero["revision"] + "/LICENSE",
                    "assetRevision": silero["revision"], "assetPath": silero["modelPath"],
                    "assetGitBlob": silero["modelGitBlob"], "assetSha256": silero["modelSha256"]})
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as executor:
        downloaded = dict(zip(sorted({item["source"] for item in entries}), executor.map(
            fetch, sorted({item["source"] for item in entries}))))
    for item in entries:
        raw = downloaded[item["source"]]
        if len(raw) != item["size"] or hashlib.sha1(b"blob " + str(len(raw)).encode()
                + b"\0" + raw).hexdigest() != item["gitBlob"]:
            raise ValueError("Official tree/blob bytes mismatch")
        item["sha256"] = hashlib.sha256(raw).hexdigest()
    for relative in sorted({item["file"] for item in entries}):
        item = next(entry for entry in entries if entry["file"] == relative)
        target = folder / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("xb") as stream:
            stream.write(downloaded[item["source"]])
    with output.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump({"supplementVersion": 1, "legalApproval": False, "texts": entries}, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"components": len({(item["package"], item["version"]) for item in entries}),
                      "texts": len({item["file"] for item in entries}), "supplements": str(output),
                      "legalApproval": False}, indent=2))


def collect_attributions():
    folder = ROOT / "licenses/native"
    destination = folder / "attributions.json"
    if destination.exists():
        raise ValueError("Existing copyright attribution artifact preserved")
    supplements = json.loads((folder / "supplements.json").read_bytes())
    names = {"const-field-offset": "src/lib.rs", "const-field-offset-macro": "macro.rs",
             "vtable": "src/lib.rs", "vtable-macro": "macro.rs"}
    rows = []
    for name, source_path in sorted(names.items()):
        entry = next(row for row in supplements["texts"] if row["package"] == name)
        vcs = entry["publishedVcs"]
        url = "https://raw.githubusercontent.com/slint-ui/slint/" + vcs["git"]["sha1"] + "/"
        url += vcs["path_in_vcs"] + "/" + source_path
        raw = fetch(url)
        header = b"".join(raw.splitlines(keepends=True)[:2])
        if b"Copyright" not in header or b"SPDX-License-Identifier: MIT OR Apache-2.0" not in header:
            raise ValueError("Official helper source has no expected copyright/SPDX header")
        rows.append({"package": name, "version": entry["version"], "registryChecksum": entry["registryChecksum"],
                     "source": url, "cratePath": source_path, "sourceSize": len(raw),
                     "sourceSha256": hashlib.sha256(raw).hexdigest(),
                     "sourceGitBlob": hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest(),
                     "headerBase64": base64.b64encode(header).decode(),
                     "headerSha256": hashlib.sha256(header).hexdigest(), "byteRange": [0, len(header)]})
    with destination.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump({"attributionVersion": 1, "scope": "exact first two upstream source-header lines; no licence template modification",
                   "attributions": rows}, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"attributions": len(rows), "artifact": str(destination)}, indent=2))


if __name__ == "__main__":
    main()
