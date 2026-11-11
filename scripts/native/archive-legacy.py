import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile


ROOT = Path(__file__).resolve().parents[2]
COMMIT = "c5d07348880555a3a22b781d6b810a01e4baff5d"
ARCHIVE = ROOT / "legacy/zabor-electron-c5d07348.zip"
MANIFEST = ROOT / "legacy/manifest.json"


def git(*arguments):
    parts = ("git", *arguments)
    if any(any(character in part for character in '\"%\r\n') for part in parts):
        raise ValueError("Invalid archive command argument")
    command = " ".join('"' + part + '"' for part in parts)
    return subprocess.run('cmd.exe /d /v:off /s /c "' + command + '"', cwd=ROOT,
                          check=True, capture_output=True).stdout


def verify():
    tracked = {}
    for record in git("ls-tree", "-r", "-z", COMMIT).split(b"\0"):
        if not record:
            continue
        header, path = record.split(b"\t", 1)
        mode, kind, blob = header.decode().split()
        name = path.decode("utf-8")
        if kind != "blob" or mode != "100644":
            raise ValueError("Legacy snapshot must contain regular Git files")
        if name.startswith(".env") and name != ".env.example":
            raise ValueError("Secret environment file cannot enter legacy archive")
        if Path(name).suffix.lower() in (".pfx", ".p12", ".pem", ".key"):
            raise ValueError("Key material cannot enter legacy archive")
        tracked[name] = blob
    with zipfile.ZipFile(ARCHIVE) as archive:
        names = [entry.filename for entry in archive.infolist() if not entry.is_dir()]
        if len(names) != len(tracked) or set(names) != set(tracked):
            raise ValueError("Legacy archive file inventory differs from Git")
        files = []
        for name in sorted(names):
            raw = archive.read(name)
            blob = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
            if blob != tracked[name]:
                raise ValueError("Legacy source bytes differ from Git: " + name)
            files.append({"path": name, "gitBlob": blob, "size": len(raw),
                          "sha256": hashlib.sha256(raw).hexdigest()})
    with ARCHIVE.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return {"version": 1, "sourceCommit": COMMIT, "archive": ARCHIVE.name,
            "archiveSha256": digest,
            "fileCount": len(files), "files": files, "originalProjectModified": False}


def main():
    parser = argparse.ArgumentParser(description="Preserve the exact legacy Electron source without user profiles or dependencies")
    parser.add_argument("operation", choices=("archive", "check"))
    args = parser.parse_args()
    if args.operation == "archive":
        if ARCHIVE.exists() or MANIFEST.exists():
            raise ValueError("Existing legacy archive preserved")
        ARCHIVE.parent.mkdir(exist_ok=True)
        git("-c", "core.autocrlf=false", "archive", "--format=zip", "--output=" + str(ARCHIVE), COMMIT)
        result = verify()
        with MANIFEST.open("x", encoding="utf-8", newline="\n") as stream:
            json.dump(result, stream, indent=2, ensure_ascii=False)
            stream.write("\n")
    else:
        result = verify()
        if json.loads(MANIFEST.read_bytes()) != result:
            raise ValueError("Legacy manifest mismatch")
    print(json.dumps({"sourceCommit": COMMIT, "files": result["fileCount"],
                      "archiveSha256": result["archiveSha256"], "verified": True}))


if __name__ == "__main__":
    main()
