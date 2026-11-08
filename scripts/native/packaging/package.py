import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import struct
import subprocess
import tempfile
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[3]
POLICY = json.loads(Path(__file__).with_name("policy.json").read_text(encoding="utf-8"))
MAX_PACKAGE = 1024 * 1024 * 1024


def digest(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def load_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("Duplicate JSON field")
            result[key] = value
        return result

    def invalid_constant(value):
        raise ValueError("Non-finite JSON number: " + value)

    return json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid_constant)


def run(parts, cwd=ROOT, env=None):
    if any(re.search(r'["%\r\n]', str(part)) for part in parts):
        raise ValueError("Unsupported command argument")
    command = " ".join('"' + str(part) + '"' for part in parts)
    result = subprocess.run('cmd.exe /d /v:off /s /c "' + command + '"',
                            cwd=cwd, env=env, capture_output=True, timeout=1800)
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace")[-8000:])
    return result.stdout.decode("utf-8", errors="strict")


def safe_name(name):
    path = PurePosixPath(name)
    return (bool(name) and len(name) <= 240 and not path.is_absolute()
            and "\\" not in name and ":" not in name and str(path) == name
            and all(re.fullmatch(r"[A-Za-z0-9_.-]+", part) and part not in (".", "..")
                    and not part.endswith((".", " "))
                    and part.split(".")[0].upper() not in
                    {"CON", "PRN", "AUX", "NUL", *[f"COM{i}" for i in range(1, 10)],
                     *[f"LPT{i}" for i in range(1, 10)]} for part in path.parts))


def files_in(root):
    result = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink() or path.is_junction():
            raise ValueError("Package cannot contain links or junctions")
        if path.is_file():
            name = path.relative_to(root).as_posix()
            if not safe_name(name):
                raise ValueError(f"Unsafe package path: {name}")
            result.append((name, path))
    if len({name.lower() for name, _ in result}) != len(result):
        raise ValueError("Case-insensitive package collision")
    return result


def inspect_pe(data):
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("Missing PE executable header")
    offset = struct.unpack_from("<I", data, 60)[0]
    if offset + 264 > len(data) or data[offset:offset + 4] != b"PE\0\0":
        raise ValueError("Truncated PE executable")
    machine, count = struct.unpack_from("<HH", data, offset + 4)
    size = struct.unpack_from("<H", data, offset + 20)[0]
    optional = offset + 24
    if machine != 0x8664 or size < 240 or struct.unpack_from("<H", data, optional)[0] != 0x20B:
        raise ValueError("Native package requires PE32+ AMD64")
    if struct.unpack_from("<II", data, optional + 112 + 14 * 8) != (0, 0):
        raise ValueError("Managed CLR executable is forbidden")
    table = optional + size
    if not 0 < count <= 96 or table + count * 40 > len(data):
        raise ValueError("Invalid PE sections")
    sections = []
    for index in range(count):
        virtual_size, rva, raw_size, raw = struct.unpack_from("<IIII", data, table + index * 40 + 8)
        if raw + raw_size > len(data):
            raise ValueError("Truncated PE section")
        sections.append((rva, min(virtual_size or raw_size, raw_size), raw))

    def locate(rva, length):
        for start, size, raw in sections:
            if start <= rva and rva + length <= start + size:
                return raw + rva - start
        raise ValueError("PE RVA outside file-backed section")

    imports = set()
    for directory, width in [(1, 20), (13, 32)]:
        rva, length = struct.unpack_from("<II", data, optional + 112 + directory * 8)
        if rva == length == 0:
            continue
        if not rva or length < width or length > 1024 * 1024:
            raise ValueError("Invalid PE import directory")
        terminated = False
        for delta in range(0, length - width + 1, width):
            descriptor = data[locate(rva + delta, width):][:width]
            if descriptor == bytes(width):
                terminated = True
                break
            if directory == 13 and struct.unpack_from("<I", descriptor)[0] != 1:
                raise ValueError("Unsupported VA-based delay imports")
            name_rva = struct.unpack_from("<I", descriptor, 12 if directory == 1 else 4)[0]
            name_offset = locate(name_rva, 1)
            end = data.find(b"\0", name_offset, min(name_offset + 256, len(data)))
            if end < 0:
                raise ValueError("Unterminated PE import name")
            name = data[name_offset:end].decode("ascii").lower()
            if not re.fullmatch(r"[a-z0-9_.-]+", name):
                raise ValueError("Unsafe PE import name")
            imports.add(name)
        if not terminated:
            raise ValueError("Unterminated PE import table")
    return sorted(imports)


def check_composition(entries, neural):
    names = {name.lower() for name in entries}
    expected = {"zabor-native.exe"}
    if neural:
        expected.update(["onnxruntime.dll", "onnxruntime_providers_shared.dll"])
    if any(name not in expected and name != "build-provenance.json" and not name.startswith("licenses/")
           for name in names):
        raise ValueError("Unexpected native payload file")
    if any(name.endswith((".py", ".js", ".ts", ".cjs", ".mjs", ".bat", ".cmd", ".ps1")) for name in names):
        raise ValueError("Runtime scripting payload is forbidden")
    binaries = {name for name in names if name.endswith((".exe", ".dll", ".lib", ".wasm", ".asar"))}
    if binaries != expected:
        raise ValueError(f"Unexpected or missing native binaries: {sorted(binaries ^ expected)}")
    if any(PurePosixPath(name).name in POLICY["forbiddenNames"] for name in names):
        raise ValueError("Electron/WebView/managed reference helper is forbidden")
    imports = {}
    for name in sorted(binaries):
        imports[name] = inspect_pe(entries[name])
        for dependency in imports[name]:
            system = (dependency in POLICY["systemDlls"] or dependency.startswith("api-ms-win-")
                      or dependency.startswith("ext-ms-win-"))
            if not system and dependency not in names:
                raise ValueError(f"Unbundled non-system PE import: {name}: {dependency}")
    if neural:
        for name in ["onnxruntime.dll", "onnxruntime_providers_shared.dll"]:
            if hashlib.sha256(entries[name]).hexdigest() != POLICY["onnxFiles"]["native/" + name]:
                raise ValueError("ONNX Runtime must be exactly 1.31.1 / API 31")
    return imports


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")


def cargo_path():
    path = Path.home() / ".cargo/bin/cargo.exe"
    if not path.is_file():
        raise ValueError("Cargo executable unavailable")
    return str(path)


def metadata(neural):
    args = [cargo_path(), "metadata", "--manifest-path", "rust/Cargo.toml", "--locked", "--offline",
            "--filter-platform", POLICY["target"], "--format-version", "1"]
    if not neural:
        args += ["--no-default-features", "--features", "zabor-native/native-media"]
    return json.loads(run(args))


def collect_licenses(destination, data, cache, neural, supplements_path=None):
    locked = tomllib.loads((ROOT / "rust/Cargo.lock").read_text(encoding="utf-8"))
    checksums = {(item["name"], item["version"], item.get("source")): item.get("checksum")
                 for item in locked["package"]}
    supplements = {}
    if supplements_path:
        supplements_path = supplements_path.resolve(strict=True)
        supplemental = load_json(supplements_path.read_text(encoding="utf-8"))
        if supplemental.get("supplementVersion") != 1 or not isinstance(supplemental.get("texts"), list):
            raise ValueError("Invalid local licence supplement manifest")
        for item in supplemental["texts"]:
            source = (supplements_path.parent / item["file"]).resolve(strict=True)
            if (not re.fullmatch(r"[0-9a-f]{64}", item["sha256"]) or item["sha256"] == "0" * 64
                    or digest(source) != item["sha256"]
                    or not re.fullmatch(r"https://(?:github\.com/[^/]+/[^/]+/blob|raw\.githubusercontent\.com/[^/]+/[^/]+)/[0-9a-f]{40}/[^?#]+", item["source"])):
                raise ValueError("Licence supplement requires exact local bytes and immutable upstream provenance")
            supplements.setdefault((item["package"], item["version"]), []).append((source, item["source"]))
    packages = {item["id"]: item for item in data["packages"]}
    nodes = {item["id"]: item for item in data["resolve"]["nodes"]}
    start = next(key for key, item in packages.items() if item["name"] == "zabor-native")
    closure, pending = set(), [start]
    while pending:
        key = pending.pop()
        if key in closure:
            continue
        closure.add(key)
        pending += [item["pkg"] for item in nodes[key]["deps"]
                    if any(kind["kind"] != "dev" for kind in item["dep_kinds"])]
    inventory = []

    def copy_text(source, name):
        if not 0 < source.stat().st_size <= 16 * 1024 * 1024:
            raise ValueError("Invalid licence text size")
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        return {"path": "licenses/" + name, "sha256": digest(target),
                "sourcePath": str(source), "sourceSha256": digest(source)}

    for key in sorted(closure):
        package = packages[key]
        folder = Path(package["manifest_path"]).parent
        candidates = set()
        for directory, children, names in os.walk(folder):
            depth = len(Path(directory).relative_to(folder).parts)
            children[:] = [] if depth >= 2 else [name for name in children if name not in
                                                ("target", ".git", "node_modules", "test", "tests")]
            for name in names:
                path = Path(directory) / name
                if not path.is_symlink() and re.match(
                        r"^(license|licence|copying|notice|copyright|thirdparty)", name, re.I):
                    candidates.add(path)
        if package.get("license_file"):
            source = (folder / package["license_file"]).resolve()
            if not source.is_relative_to(folder.resolve()) or not source.is_file():
                raise ValueError(f"Invalid license-file: {package['name']}")
            candidates.add(source)
        if not candidates and package["source"] is None and folder.is_relative_to(ROOT / "rust/crates"):
            candidates.add(ROOT / "LICENSE")
        extra = supplements.get((package["name"], package["version"]), [])
        candidates.update(source for source, _ in extra)
        expression = package.get("license") or ("license-file" if package.get("license_file") else "UNKNOWN")
        identifier = re.sub(r"[^A-Za-z0-9_.-]", "_", package["name"] + "-" + package["version"])
        identifier += "-" + hashlib.sha256(key.encode()).hexdigest()[:12]
        texts = [copy_text(source, identifier + "/" + str(index) + "-" + re.sub(
            r"[^A-Za-z0-9_.-]", "_", source.name)) for index, source in enumerate(sorted(candidates))]
        inventory.append({"name": package["name"], "version": package["version"],
                          "source": package["source"] or "local-workspace-or-vendor",
                          "registryChecksum": checksums.get((package["name"], package["version"], package["source"])),
                          "supplementProvenance": [provenance for _, provenance in extra],
                          "licenseExpression": expression,
                          "selectedLicense": "GPL-3.0-only" if package["name"].startswith(("slint", "i-slint"))
                          and "GPL-3.0-only" in expression else expression,
                          "noticeStatus": "present" if texts else "upstream-text-required",
                          "authors": package.get("authors", []), "repository": package.get("repository"),
                          "localPatchSha256": digest(folder / "ZABOR-PATCH.md")
                          if (folder / "ZABOR-PATCH.md").is_file() else None,
                          "manifestSha256": digest(Path(package["manifest_path"])), "texts": texts})
    sdk = cache / "webrtc-win-x64-a545de4"
    sdk_license = sdk / "LICENSE.md"
    if not sdk_license.is_file():
        raise ValueError("Pinned WebRTC SDK LICENSE.md missing or ambiguous")
    inventory.append({"name": "webrtc-sdk", "version": "a545de4", "source": "local-pinned-sdk",
                      "licenseExpression": "upstream multi-component LICENSE.md",
                      "texts": [copy_text(sdk_license, "webrtc-sdk-a545de4/LICENSE.md")],
                      "libraries": [{"name": path.relative_to(sdk).as_posix(), "sha256": digest(path)}
                                    for path in sorted((sdk / "lib").glob("*.lib"))]})
    if not inventory[-1]["libraries"]:
        raise ValueError("Pinned WebRTC static libraries missing")
    inventory.append({"name": "libclang-build-tool", "version": "18.1.1", "source": "pinned PyPI Windows x64 wheel",
                      "licenseExpression": "Apache-2.0 WITH LLVM-exception; build-only, not runtime",
                      "texts": [copy_text(cache / "libclang-win-x64-18.1.1/licenses/LICENSE-0.txt",
                                          "libclang-build-tool-18.1.1/LICENSE-0.txt")]})
    if neural:
        runtime = cache / "onnxruntime-win-x64-1.31.1"
        inventory.append({"name": "onnxruntime", "version": "1.31.1", "api": 31,
                          "source": "Microsoft.ML.OnnxRuntime/1.31.1 pinned NuGet package",
                          "licenseExpression": "MIT and bundled third-party notices",
                          "texts": [copy_text(runtime / "licenses" / name, "onnxruntime-1.31.1/" + name)
                                    for name in ["LICENSE", "ThirdPartyNotices.txt"]]})
        deep_filter = next(item for item in inventory if item["name"] == "deep_filter")
        inventory.append({"name": "deepfilternet3-model", "version": "d375b2d8309e0935d165700c91da9de862a99c31",
                          "source": "existing repository model; THIRD-PARTY-NOTICES.md upstream provenance",
                          "assetSha256": digest(ROOT / "src/renderer/public/deepfilternet3/models/DeepFilterNet3_onnx.tar.gz"),
                          "licenseExpression": "MIT OR Apache-2.0", "texts": deep_filter["texts"]})
        silero_extra = supplements.get(("silero-vad-model", "existing-local-model"), [])
        inventory.append({"name": "silero-vad-model", "version": "existing-local-model",
                          "source": "https://github.com/snakers4/silero-vad; existing repository model",
                          "assetSha256": digest(ROOT / "src/renderer/public/silero_vad.onnx"),
                          "licenseExpression": "MIT", "supplementProvenance": [url for _, url in silero_extra],
                          "texts": [copy_text(path, "silero-vad-model/" + str(index) + "-" + path.name)
                                    for index, (path, _) in enumerate(silero_extra)]})
    inventory.append({"name": "zabor-notices", "version": POLICY["version"],
                      "source": "repository notices: Phosphor, Silero, DFN/RNNoise models, SQLite",
                      "licenseExpression": "see individual component notices; not legal approval",
                      "texts": [copy_text(ROOT / "THIRD-PARTY-NOTICES.md", "ZABOR-THIRD-PARTY-NOTICES.md"),
                                copy_text(ROOT / "LICENSE", "ZABOR-LICENSE")]})
    rustc = str(Path(cargo_path()).with_name("rustc.exe"))
    sysroot = Path(run([rustc, "--print", "sysroot"]).strip())
    rust_licenses = sysroot / "share/doc/rust/licenses"
    if not rust_licenses.is_dir():
        raise ValueError("Rust toolchain licence directory missing")
    inventory.append({"name": "rust-toolchain-runtime", "version": run([rustc, "--version"]).strip(),
                      "source": "installed toolchain licence bundle (conservative superset)",
                      "licenseExpression": "see bundled Rust/LLVM runtime notices",
                      "texts": [copy_text(path, "rust-toolchain/" + path.relative_to(rust_licenses).as_posix())
                                for path in sorted(rust_licenses.rglob("*")) if path.is_file()]})
    missing = [item["name"] + "@" + item["version"] for item in inventory
               if not item["texts"] or item["licenseExpression"] == "UNKNOWN"]
    write_json(destination / "inventory.json", {"inventoryVersion": 1, "target": POLICY["target"],
               "cargoLockSha256": digest(ROOT / "rust/Cargo.lock"), "packages": inventory,
               "missingUpstreamTexts": missing, "complete": not missing,
               "scope": "Windows target filtered transitive normal/build closure; conservative superset",
               "legalApproval": False, "nativeClientReady": False})
    return len(inventory)


def check_cache(cache, neural):
    for name in ["webrtc-win-x64-a545de4", "libclang-win-x64-18.1.1"]:
        if not (cache / name).is_dir():
            raise ValueError(f"Offline pinned cache missing: {name}")
    sdk = cache / "webrtc-win-x64-a545de4"
    if (digest(sdk / "lib/webrtc.lib") != POLICY["webRtcLibrarySha256"]
            or digest(sdk / "LICENSE.md") != POLICY["webRtcLicenseSha256"]):
        raise ValueError("Pinned WebRTC SDK static library or licence hash mismatch")
    for name, expected in POLICY["libclangFiles"].items():
        if digest(cache / "libclang-win-x64-18.1.1" / name) != expected:
            raise ValueError("Pinned libclang cache hash mismatch")
    if neural:
        for name, expected in POLICY["onnxFiles"].items():
            if digest(cache / "onnxruntime-win-x64-1.31.1" / name) != expected:
                raise ValueError("Pinned ONNX Runtime cache hash mismatch")
        model = cache / "rnnoise-4ac81c5c/rnnoise-modeltar.gz"
        if digest(model) != "4ac81c5c0884ec4bd5907026aaae16209b7b76cd9d7f71af582094a2f98f4b43":
            raise ValueError("Pinned RNNoise model hash mismatch")


def make_archive(stage, output, neural, fixture=False):
    if output.exists():
        raise ValueError("Refusing to overwrite existing package")
    if (stage / "native-package.json").exists():
        raise ValueError("Stage already contains a package manifest")
    entries = {name.lower(): path.read_bytes() for name, path in files_in(stage)}
    imports = check_composition(entries, neural)
    if sum(len(value) for value in entries.values()) > MAX_PACKAGE:
        raise ValueError("Package exceeds local size limit")
    commit = run(["git", "rev-parse", "HEAD"]).strip()
    manifest = {"manifestVersion": 1, "version": POLICY["version"], "channel": "native-beta",
                "profile": POLICY["profile"], "platform": "win32", "architecture": "x64",
                "commit": commit, "dataSchema": 1, "minimumDataSchema": 1, "maximumDataSchema": 1,
                "neuralDsp": neural, "installationRoute": POLICY["installationRoute"],
                "localFixture": fixture, "nativeClientReady": False, "signatureRequired": True,
                "files": [{"path": name, "size": path.stat().st_size, "sha256": digest(path)}
                          for name, path in files_in(stage)]}
    write_json(stage / "native-package.json", manifest)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".native-zip-", dir=output.parent) as temporary:
        archive = Path(temporary) / "package.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as zipped:
            for name, path in files_in(stage):
                info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o100644 << 16
                zipped.writestr(info, path.read_bytes())
        verify_archive(archive)
        shutil.move(archive, output)
    return {"package": str(output), "sha256": digest(output), "peImports": imports,
            "nativeClientReady": False, "signed": False, "installationAllowed": False}


def verify_archive(path):
    if path.stat().st_size > MAX_PACKAGE:
        raise ValueError("Package exceeds size limit")
    with zipfile.ZipFile(path) as archive:
        infos = archive.infolist()
        names = [item.filename for item in infos]
        if (len(infos) > 20000 or sum(item.file_size for item in infos) > MAX_PACKAGE
                or len({name.lower() for name in names}) != len(names)
                or any(not safe_name(name) for name in names)
                or any((item.external_attr >> 16) & 0o170000 not in (0, 0o100000) or item.flag_bits & 1
                       for item in infos)):
            raise ValueError("Unsafe or oversized ZIP composition")
        raw = archive.read("native-package.json")
        if len(raw) > 4 * 1024 * 1024:
            raise ValueError("Oversized package manifest")
        manifest = load_json(raw)
        for key in ["manifestVersion", "version", "profile", "platform", "architecture", "installationRoute"]:
            if manifest.get(key) != POLICY[key]:
                raise ValueError(f"Native manifest policy mismatch: {key}")
        if manifest.get("channel") != "native-beta" or manifest.get("signatureRequired") is not True:
            raise ValueError("Native manifest channel/signature mismatch")
        if not isinstance(manifest.get("neuralDsp"), bool) or not isinstance(manifest.get("localFixture"), bool):
            raise ValueError("Native manifest flags invalid")
        files = manifest.get("files")
        if not isinstance(files, list) or len(files) != len(names) - 1:
            raise ValueError("Unlisted ZIP entries")
        if len({item["path"].lower() for item in files}) != len(files):
            raise ValueError("Duplicate manifest files")
        if set(names) != {"native-package.json", *[item["path"] for item in files]}:
            raise ValueError("ZIP/manifest file set differs")
        entries = {}
        for item in files:
            data = archive.read(item["path"])
            if type(item["size"]) is not int or len(data) != item["size"] or hashlib.sha256(data).hexdigest() != item["sha256"]:
                raise ValueError("Package file hash/length mismatch")
            entries[item["path"].lower()] = data
        imports = check_composition(entries, manifest["neuralDsp"])
        inventory = load_json(entries["licenses/inventory.json"])
        if (inventory.get("inventoryVersion") != 1 or inventory.get("target") != POLICY["target"]
                or inventory.get("complete") is not True or not inventory.get("packages")):
            raise ValueError("Missing license inventory")
        for package in inventory["packages"]:
            if not package.get("texts") or not package.get("licenseExpression") or package["licenseExpression"] == "UNKNOWN":
                raise ValueError("Missing license texts/expression")
            for text in package["texts"]:
                if hashlib.sha256(entries[text["path"].lower()]).hexdigest() != text["sha256"]:
                    raise ValueError("License inventory hash mismatch")
    return {"package": str(path), "sha256": digest(path), "files": len(files), "peImports": imports,
            "localIntegrity": "passed", "nativeClientReady": False, "installationAllowed": False}


def compile_binary(args):
    target = args.target_dir.resolve()
    cache = args.cache_root.resolve()
    if not target.is_relative_to(ROOT) or target == ROOT or target == cache:
        raise ValueError("Build target must be a separate directory in this worktree")
    neural = not args.neural_disabled
    check_cache(cache, neural)
    env = {name: os.environ[name] for name in (
        "PATH", "SystemRoot", "WINDIR", "ComSpec", "TEMP", "TMP", "USERPROFILE", "HOMEDRIVE", "HOMEPATH",
        "LOCALAPPDATA", "APPDATA", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "NUMBER_OF_PROCESSORS",
        "PROCESSOR_ARCHITECTURE", "VCINSTALLDIR", "VSINSTALLDIR", "WindowsSdkDir", "WindowsSDKVersion",
        "INCLUDE", "LIB", "LIBPATH") if name in os.environ}
    env.update({"CARGO_TARGET_DIR": str(target), "CARGO_NET_OFFLINE": "true",
                "RUSTFLAGS": "-C target-feature=+crt-static",
                "LK_CUSTOM_WEBRTC": str(cache / "webrtc-win-x64-a545de4"),
                "LIBCLANG_PATH": str(cache / "libclang-win-x64-18.1.1/native"),
                "ORT_LIB_LOCATION": str(cache / "onnxruntime-win-x64-1.31.1/native"),
                "ORT_PREFER_DYNAMIC_LINK": "1",
                "RNNOISE_MODEL_PATH": str(cache / "rnnoise-4ac81c5c/rnnoise-modeltar.gz")})
    host = run([str(Path(cargo_path()).with_name("rustc.exe")), "-vV"])
    if "host: " + POLICY["target"] not in host:
        raise ValueError("This local build requires the Windows x64 MSVC host toolchain")
    parameters = [cargo_path(), "build", "--manifest-path", "rust/Cargo.toml", "-p", "zabor-native",
                  "--locked", "--offline", "-j", "4"]
    if args.release:
        parameters += ["--release"]
    if not neural:
        parameters += ["--no-default-features", "--features", "native-media"]
    run(parameters, env=env)
    binary = target / ("release" if args.release else "debug") / "zabor-native.exe"
    imports = inspect_pe(binary.read_bytes())
    if args.clippy:
        parameters = [cargo_path(), "clippy", "--manifest-path", "rust/Cargo.toml", "-p", "zabor-native",
                      "--lib", "--locked", "--offline", "-j", "4"]
        if not neural:
            parameters += ["--no-default-features", "--features", "native-media"]
        run(parameters + ["--", "-D", "warnings"], env=env)
    return {"binary": str(binary), "sha256": digest(binary), "imports": imports,
            "neuralDsp": neural, "staticCrt": True, "nativeClientReady": False,
            "clippy": "passed" if args.clippy else "not-run",
            "installationAllowed": False}


def build(args):
    if args.output.exists() or args.output.suffix.lower() != ".zip":
        raise ValueError("Output must be a new local .zip package")
    cache = args.cache_root.resolve()
    neural = not args.neural_disabled
    check_cache(cache, neural)
    data = metadata(neural)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".native-package-", dir=args.output.parent) as temporary:
        stage = Path(temporary)
        count = collect_licenses(stage / "licenses", data, cache, neural, args.license_supplements)
        inventory = json.loads((stage / "licenses/inventory.json").read_text())
        if inventory["missingUpstreamTexts"]:
            raise ValueError("Missing upstream licence texts: " + ", ".join(inventory["missingUpstreamTexts"]))
        result = compile_binary(args)
        shutil.copyfile(result["binary"], stage / "zabor-native.exe")
        if neural:
            for name in ["onnxruntime.dll", "onnxruntime_providers_shared.dll"]:
                shutil.copyfile(cache / "onnxruntime-win-x64-1.31.1/native" / name, stage / name)
        write_json(stage / "build-provenance.json", {"cargoLockSha256": digest(ROOT / "rust/Cargo.lock"),
                   "policySha256": digest(Path(__file__).with_name("policy.json")),
                   "profile": "release" if args.release else "debug", "target": POLICY["target"],
                   "staticCrt": True, "neuralDsp": neural, "licenseComponents": count,
                   "sourceStatus": run(["git", "status", "--porcelain", "--untracked-files=no"]),
                   "nativeClientReady": False, "acceptanceApproval": None})
        return make_archive(stage, args.output.resolve(), neural)


def main():
    parser = argparse.ArgumentParser(description="Local-only offline native Windows package builder/verifier")
    commands = parser.add_subparsers(dest="command", required=True)
    command = commands.add_parser("build")
    command.add_argument("--cache-root", type=Path, required=True)
    command.add_argument("--target-dir", type=Path, required=True)
    command.add_argument("--output", type=Path, required=True)
    command.add_argument("--release", action="store_true")
    command.add_argument("--neural-disabled", action="store_true")
    command.add_argument("--clippy", action="store_true")
    command.add_argument("--license-supplements", type=Path)
    command = commands.add_parser("build-binary")
    command.add_argument("--cache-root", type=Path, required=True)
    command.add_argument("--target-dir", type=Path, required=True)
    command.add_argument("--release", action="store_true")
    command.add_argument("--neural-disabled", action="store_true")
    command.add_argument("--clippy", action="store_true")
    command = commands.add_parser("verify")
    command.add_argument("package", type=Path)
    command = commands.add_parser("inventory")
    command.add_argument("--cache-root", type=Path, required=True)
    command.add_argument("--output", type=Path, required=True)
    command.add_argument("--neural-disabled", action="store_true")
    command.add_argument("--license-supplements", type=Path)
    args = parser.parse_args()
    try:
        if os.name != "nt":
            raise ValueError("Native packaging requires Windows")
        if args.command == "build":
            result = build(args)
        elif args.command == "build-binary":
            result = compile_binary(args)
        elif args.command == "verify":
            result = verify_archive(args.package)
        else:
            if args.output.exists():
                raise ValueError("Refusing to overwrite license inventory")
            args.output.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.TemporaryDirectory(prefix=".native-licenses-", dir=args.output.parent) as temporary:
                count = collect_licenses(Path(temporary), metadata(not args.neural_disabled),
                                         args.cache_root.resolve(), not args.neural_disabled, args.license_supplements)
                shutil.move(temporary, args.output)
            result = {"licenseComponents": count, "inventory": str(args.output / "inventory.json"),
                      "missingUpstreamTexts": json.loads((args.output / "inventory.json").read_text())["missingUpstreamTexts"],
                      "legalApproval": False, "nativeClientReady": False}
        print(json.dumps(result, indent=2))
    except (ValueError, OSError, RuntimeError, KeyError, TypeError, zipfile.BadZipFile) as error:
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
