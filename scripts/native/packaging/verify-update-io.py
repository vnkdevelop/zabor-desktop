import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[3]


def run_line(line, cwd, env=None, timeout=1800):
    result = subprocess.run('cmd.exe /d /v:off /s /c "' + line + '"', cwd=cwd, env=env,
                            capture_output=True, timeout=timeout)
    text = result.stdout.decode("utf-8", errors="replace") + result.stderr.decode("utf-8", errors="replace")
    if result.returncode:
        raise RuntimeError(text[-16000:])
    return text


def quoted(parts):
    if any('"' in str(value) or '%' in str(value) or '\n' in str(value) for value in parts):
        raise ValueError("Unsupported command argument")
    return " ".join('"' + str(value) + '"' for value in parts)


def main():
    parser = argparse.ArgumentParser(description="External frozen-dependency native update IO sandbox harness")
    parser.add_argument("--keep-failure-log", type=Path)
    parser.add_argument("--workspace", type=Path)
    args = parser.parse_args()
    original_lock = (ROOT / "rust/Cargo.lock").read_bytes()
    original = tomllib.loads(original_lock.decode())
    packages = original["package"]
    wanted = {"fs2": "0.4.", "getrandom": "0.3.", "p256": "0.13.", "reqwest": "0.12.",
              "semver": "1.", "serde": "1.", "serde_json": "1.", "sha2": "0.10.", "tempfile": "3.",
              "tokio": "1.", "tokio-util": "0.7.", "windows-dpapi": "0.2.", "zeroize": "1.",
              "zip": "9.", "flate2": "1."}
    selected = {}
    for name, prefix in wanted.items():
        matches = [item for item in packages if item["name"] == name and item["version"].startswith(prefix)]
        if len(matches) != 1:
            raise ValueError("Ambiguous frozen dependency: " + name)
        selected[name] = matches[0]["version"]
    permitted = {(item["name"], item["version"], item.get("source")): item.get("checksum") for item in packages}
    parent = Path.home() / "AppData/Local/Temp/opencode"
    workspace_context = None
    if args.workspace:
        workspace = args.workspace.resolve()
        if workspace.parent != parent.resolve():
            raise ValueError("Reusable harness must be directly inside the approved OpenCode temp directory")
        marker = workspace / ".native-update-io-owner"
        if workspace.exists():
            if not marker.is_file() or marker.read_text(encoding="utf-8") != str(ROOT):
                raise ValueError("Reusable harness is not owned by this worktree")
        else:
            workspace.mkdir()
            marker.write_text(str(ROOT), encoding="utf-8")
        workspace_context = contextlib.nullcontext(str(workspace))
    cargo = Path.home() / ".cargo/bin/cargo.exe"
    rustfmt = cargo.with_name("rustfmt.exe")
    vswhere = Path("C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe")
    source = ROOT / "rust/crates/zabor-native/src"
    files = [source / name for name in ["release.rs", "update.rs", "update_helper.rs", "update_helper_tests.rs", "update_io.rs", "update_io_tests.rs", "update_url.rs"]]
    cpp = ROOT / "rust/crates/zabor-native/update-helper/native_update_helper.cpp"
    source_hashes = {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                     for path in [*files, cpp, Path(__file__).resolve()]}
    log = ""
    try:
        with (workspace_context or tempfile.TemporaryDirectory(prefix="native-update-io-harness-", dir=parent)) as directory:
            harness = Path(directory)
            test_directory = harness / "sandboxes"
            test_directory.mkdir(exist_ok=True)
            install = run_line(quoted([vswhere, "-latest", "-products", "*", "-requires",
                                       "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"]), harness).strip()
            vcvars = Path(install) / "VC/Auxiliary/Build/vcvars64.bat"
            helper = harness / "native-update-helper.exe"
            log += run_line('call "' + str(vcvars) + '" && ' + quoted([
                "cl.exe", "/nologo", "/std:c++20", "/EHsc", "/W4", "/WX", "/MT", "/O2",
                cpp, "/Fe:" + str(helper), "/link", "advapi32.lib", "wintrust.lib", "crypt32.lib"]), harness)
            manifest = ['[package]', 'name = "zabor-update-io-harness"', 'version = "0.1.0"', 'edition = "2021"',
                        '[lib]', 'path = "lib.rs"', '[dependencies]']
            features = {
                "serde": ', features = ["derive"]',
                "zeroize": ', features = ["serde"]',
                "tokio": ', features = ["rt-multi-thread", "macros", "net", "sync", "time", "io-util"]',
                "reqwest": ', default-features = false, features = ["json", "rustls-tls-native-roots"]',
                "zip": ', default-features = false, features = ["deflate-flate2"]',
                "flate2": ', features = ["rust_backend"]',
            }
            for name, version in selected.items():
                manifest.append(name + ' = { version = "=' + version + '"' + features.get(name, "") + ' }')
            (harness / "Cargo.toml").write_text("\n".join(manifest) + "\n", encoding="utf-8")
            (harness / "Cargo.lock").write_bytes(original_lock)
            (harness / ".cargo").mkdir(exist_ok=True)
            (harness / ".cargo/config.toml").write_text('[target.x86_64-pc-windows-msvc]\nrustflags = ["-C", "target-feature=+crt-static"]\n', encoding="utf-8")
            modules = ['#![forbid(unsafe_code)]']
            for name in ["release", "update", "update_helper", "update_io"]:
                modules += ['#[path = "' + (source / (name + ".rs")).as_posix() + '"]', 'pub mod ' + name + ';']
            (harness / "lib.rs").write_text("\n".join(modules) + "\n", encoding="utf-8")
            env = {name: os.environ[name] for name in ["PATH", "SystemRoot", "WINDIR", "USERPROFILE", "HOMEDRIVE",
                   "HOMEPATH", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP", "ProgramFiles", "ProgramFiles(x86)"] if name in os.environ}
            env.update({"CARGO_TARGET_DIR": str(harness / "target"), "CARGO_NET_OFFLINE": "true",
                        "ZABOR_UPDATE_TEST_DIRECTORY": str(test_directory), "ZABOR_UPDATE_TEST_HELPER": str(helper)})
            log += run_line(quoted([cargo, "metadata", "--offline", "--filter-platform", "x86_64-pc-windows-msvc",
                                    "--format-version", "1"]), harness, env)
            derived = tomllib.loads((harness / "Cargo.lock").read_text())
            for package in derived["package"]:
                if package["name"] == "zabor-update-io-harness":
                    continue
                identifier = (package["name"], package["version"], package.get("source"))
                if identifier not in permitted or package.get("checksum") != permitted[identifier]:
                    raise ValueError("External harness selected a dependency outside frozen root lock: " + package["name"])
            log += run_line(quoted([rustfmt, "--edition", "2021", "--check", *files]), harness, env)
            compiler_prefix = 'call "' + str(vcvars) + '" && '
            checks = run_line(compiler_prefix + quoted([cargo, "clippy", "--locked", "--offline", "--all-targets", "-j", "4", "--", "-D", "warnings"]), harness, env)
            log += checks
            tests = run_line(compiler_prefix + quoted([cargo, "test", "--locked", "--offline", "-j", "4", "--", "--test-threads=1"]), harness, env)
            log += tests
            if (ROOT / "rust/Cargo.lock").read_bytes() != original_lock:
                raise ValueError("Shared root lock changed")
            for name, expected in source_hashes.items():
                if hashlib.sha256((ROOT / name).read_bytes()).hexdigest() != expected:
                    raise ValueError("Source changed during verification: " + name)
            print(tests[-16000:])
            print(json.dumps({"nativeHelper": "compiled static CRT /W4 /WX", "clippy": "passed", "fmt": "passed",
                  "frozenDependencies": len(derived["package"])-1, "sharedCargoUnchanged": True,
                  "nativeClientReady": False, "scope": "external temporary native update IO sandbox only"}, indent=2))
    except Exception as error:
        if args.keep_failure_log:
            args.keep_failure_log.write_text(log + "\n" + str(error), encoding="utf-8")
        raise


if __name__ == "__main__":
    main()
