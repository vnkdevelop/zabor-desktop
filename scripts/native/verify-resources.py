import argparse
import hashlib
import json
from pathlib import Path
import re
import stat


ROOT = Path(__file__).resolve().parents[2]
RESOURCE_DIRECTORY = Path("rust/crates/zabor-native/assets")
SOURCES = {
    "catalogs/ru.json": "src/renderer/src/locales/ru.json",
    "catalogs/en.json": "src/renderer/src/locales/en.json",
    "models/DeepFilterNet3_onnx.tar.gz": "src/renderer/public/deepfilternet3/models/DeepFilterNet3_onnx.tar.gz",
    "models/silero_vad.onnx": "src/renderer/public/silero_vad.onnx",
}


def regular_file(root, relative):
    path = root
    for part in relative.parts:
        path = path / part
        info = path.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
            raise ValueError(f"Resource path is redirected: {relative.as_posix()}")
    if not stat.S_ISREG(info.st_mode):
        raise ValueError(f"Resource is not a regular file: {relative.as_posix()}")
    return path


def verify_bytes(root, relative, size, expected):
    path = regular_file(root, relative)
    if path.stat().st_size != size:
        raise ValueError(f"Resource size differs: {relative.as_posix()}")
    with path.open("rb") as stream:
        content = stream.read(size + 1)
    if len(content) != size or hashlib.sha256(content).hexdigest() != expected:
        raise ValueError(f"Resource bytes differ: {relative.as_posix()}")
    return content


def verify(root=ROOT, reference=False):
    root = Path(root).resolve(strict=True)
    manifest_path = regular_file(root, RESOURCE_DIRECTORY / "resources.json")
    with manifest_path.open("rb") as stream:
        content = stream.read(64 * 1024 + 1)
    if len(content) > 64 * 1024:
        raise ValueError("Resource manifest is too large")
    manifest = json.loads(content)
    if not isinstance(manifest, dict) or type(manifest.get("resourceVersion")) is not int or manifest["resourceVersion"] != 1:
        raise ValueError("Unsupported resource manifest")
    resources = manifest.get("resources")
    if not isinstance(resources, list) or len(resources) != len(SOURCES):
        raise ValueError("Native resource inventory is incomplete")
    checked = set()
    for item in resources:
        if not isinstance(item, dict):
            raise ValueError("Invalid resource record")
        name = item.get("path")
        if not isinstance(name, str) or name not in SOURCES or name in checked:
            raise ValueError("Unexpected or duplicate native resource")
        if item.get("sourcePath") != SOURCES[name]:
            raise ValueError(f"Unexpected reference path: {name}")
        size, expected = item.get("size"), item.get("sha256")
        if type(size) is not int or not 0 < size <= 16 * 1024 * 1024:
            raise ValueError(f"Invalid resource size: {name}")
        if not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected):
            raise ValueError(f"Invalid resource digest: {name}")
        verify_bytes(root, RESOURCE_DIRECTORY / name, size, expected)
        if reference:
            verify_bytes(root, Path(SOURCES[name]), size, expected)
        checked.add(name)
    return {"passed": True, "resources": sorted(checked), "referenceCompared": reference,
            "scope": "Pinned native resource bytes only", "legalApproval": False, "nativeClientReady": False}


def main():
    parser = argparse.ArgumentParser(description="Verify pinned native resource bytes without Node or renderer dependencies")
    parser.add_argument("--reference", action="store_true", help="Also require exact retained working-copy reference bytes")
    options = parser.parse_args()
    try:
        print(json.dumps(verify(reference=options.reference), indent=2))
    except (OSError, ValueError) as error:
        parser.exit(1, f"Native resource verification failed: {error}\n")


if __name__ == "__main__":
    main()
