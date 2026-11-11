import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description="Owned loopback hub lifetime checks")
    parser.add_argument("--attempt", type=int, default=1)
    args = parser.parse_args()
    if not 1 <= args.attempt <= 100:
        raise ValueError("Invalid evidence attempt")
    evidence = ROOT / f"migration-artifacts/native-data/hub-lifetime-2026-11-09/checks-v{args.attempt}"
    evidence.mkdir(parents=True, exist_ok=False)
    names = [
        "rust/crates/zabor-native/src/hub.rs",
        "rust/crates/zabor-native/src/hub_lifetime.rs",
        "rust/crates/zabor-native/src/protocol.rs",
        "rust/crates/zabor-native/src/shell.rs",
        "rust/crates/zabor-native/src/shell/session_io.rs",
        "rust/crates/zabor-native/src/shell/chat_io.rs",
        "rust/crates/zabor-native/src/shell/file_io.rs",
        "rust/crates/zabor-native/src/update_host.rs",
        "rust/Cargo.toml", "rust/Cargo.lock",
        "rust/crates/zabor-native/Cargo.toml", ".cargo/config.toml",
        "scripts/native/verify-hub-lifetime.py",
        "rust/crates/zabor-native/src/avatar.rs",
        "rust/crates/zabor-native/src/avatar/tests.rs",
    ]
    frozen = {name: digest(ROOT / name) for name in names}
    receipt = {"passed": False, "sourceHashes": frozen, "checks": [], "tests": []}
    env = {name: value for name, value in os.environ.items() if name.upper() in {
        "PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "USERPROFILE", "HOMEDRIVE", "HOMEPATH",
        "APPDATA", "LOCALAPPDATA", "TEMP", "TMP", "PROGRAMFILES", "PROGRAMFILES(X86)",
        "PROGRAMW6432", "PROGRAMDATA", "ALLUSERSPROFILE",
    }}
    env.update({"CARGO_NET_OFFLINE": "true", "RUSTFLAGS": "-C target-feature=+crt-static",
                "PYTHONDONTWRITEBYTECODE": "1"})
    cargo = Path.home() / ".cargo/bin/cargo.exe"
    rustfmt = cargo.with_name("rustfmt.exe")
    packages = tomllib.loads((ROOT / "rust/Cargo.lock").read_text(encoding="utf-8"))["package"]
    wanted = {"serde_json": "1.", "tokio": "1.", "tokio-util": "0.7.",
              "tokio-tungstenite": "0.28.", "reqwest": "0.12.", "futures-util": "0.3.",
              "url": "2.", "zeroize": "1.", "thiserror": "2.", "base64": "0.22.",
              "image": "0.25.", "serde": "1."}
    versions = {}
    for name, prefix in wanted.items():
        matches = [item for item in packages if item["name"] == name and item["version"].startswith(prefix)]
        if len(matches) != 1:
            raise ValueError("Ambiguous pinned dependency: " + name)
        versions[name] = matches[0]["version"]
    features = {
        "tokio": ', features = ["rt-multi-thread", "macros", "net", "sync", "time", "io-util"]',
        "tokio-tungstenite": ', features = ["rustls-tls-native-roots"]',
        "reqwest": ', default-features = false, features = ["json", "rustls-tls-native-roots"]',
        "futures-util": ', features = ["sink"]',
        "zeroize": ', features = ["serde"]',
        "serde": ', features = ["derive"]',
        "image": ', default-features = false, features = ["png", "jpeg", "gif", "webp"]',
    }
    temporary_path = None

    def check(name, arguments, cwd):
        line = subprocess.list2cmdline([str(item) for item in arguments])
        command = 'cmd.exe /d /v:off /s /c "' + line + '"'
        started = datetime.datetime.now(datetime.timezone.utc).isoformat()
        result = subprocess.run(command, cwd=cwd, env=env, capture_output=True)
        text = result.stdout.decode("utf-8", errors="replace") + result.stderr.decode("utf-8", errors="replace")
        path = evidence / (name + ".log")
        path.write_text(text, encoding="utf-8")
        receipt["checks"].append({"name": name, "command": command, "originalExitCode": result.returncode,
                                  "startedUtc": started,
                                  "completedUtc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                                  "logSha256": digest(path)})
        (evidence / "verification.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        if result.returncode:
            raise RuntimeError(name + " failed: " + text[-12000:])
        return text

    try:
        with tempfile.TemporaryDirectory(prefix="native-hub-lifetime-", dir=Path.home() / "AppData/Local/Temp/opencode") as temporary:
            harness = Path(temporary)
            temporary_path = harness
            env["CARGO_TARGET_DIR"] = str(harness / "target")
            manifest = ['[package]', 'name = "zabor-hub-lifetime-harness"', 'version = "0.1.0"',
                        'edition = "2021"', '[lib]', 'path = "lib.rs"', '[dependencies]']
            manifest += [name + ' = { version = "=' + version + '"' + features.get(name, "") + ' }'
                         for name, version in versions.items()]
            (harness / "Cargo.toml").write_text("\n".join(manifest) + "\n", encoding="utf-8")
            (harness / "Cargo.lock").write_bytes((ROOT / "rust/Cargo.lock").read_bytes())
            modules = ['#![forbid(unsafe_code)]']
            for name in ["avatar", "protocol", "hub"]:
                path = ROOT / f"rust/crates/zabor-native/src/{name}.rs"
                (harness / (name + ".rs")).write_bytes(path.read_bytes())
                modules += ['pub mod ' + name + ';']
            (harness / "hub_lifetime.rs").write_bytes((ROOT / names[1]).read_bytes())
            (harness / "avatar").mkdir()
            (harness / "avatar/tests.rs").write_bytes((ROOT / names[-1]).read_bytes())
            (harness / "lib.rs").write_text("\n".join(modules) + "\n", encoding="utf-8")
            check("lock-resolution", [cargo, "metadata", "--offline", "--format-version", "1"], harness)
            selected = tomllib.loads((harness / "Cargo.lock").read_text(encoding="utf-8"))["package"]
            permitted = {(item["name"], item["version"], item.get("source")): item.get("checksum") for item in packages}
            for item in selected:
                if item["name"] == "zabor-hub-lifetime-harness":
                    continue
                key = (item["name"], item["version"], item.get("source"))
                if key not in permitted or permitted[key] != item.get("checksum"):
                    raise ValueError("Harness dependency differs from primary lock: " + item["name"])
            receipt["dependencySubsetCount"] = len(selected) - 1
            check("affected-fmt", [rustfmt, "--edition", "2021", "--check", "--config", "skip_children=true",
                                   *[ROOT / name for name in [names[0], names[1], names[4]]]], ROOT)
            check("strict-clippy", [cargo, "clippy", "--offline", "--locked", "-j2", "--all-targets", "--", "-D", "warnings"], harness)
            text = check("hub-tests", [cargo, "test", "--offline", "--locked", "-j2", "--lib", "hub::", "--", "--test-threads=1"], harness)
            receipt["tests"] = re.findall(r"^test (hub::\S+) \.\.\. ok$", text, re.MULTILINE)
            if len(receipt["tests"]) != 8:
                raise ValueError("Expected eight actual hub test passes")
            if any(digest(ROOT / name) != value for name, value in frozen.items()):
                raise ValueError("Frozen source changed during original checks")
        receipt["passed"] = temporary_path is not None and not temporary_path.exists()
    finally:
        receipt["ownedTemporaryRemoved"] = temporary_path is not None and not temporary_path.exists()
        (evidence / "verification.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": receipt["passed"], "tests": len(receipt["tests"]),
                      "ownedTemporaryRemoved": receipt["ownedTemporaryRemoved"]}))


if __name__ == "__main__":
    main()
