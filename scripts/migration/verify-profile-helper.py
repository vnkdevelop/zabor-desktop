import argparse
import os
import pathlib
import shutil
import subprocess
import tempfile
import tomllib

def command_part(value):
    value = os.fspath(value)
    if not isinstance(value, str) or not value or any(
        ord(character) < 32 or ord(character) == 127
        or character in '&|<>^%"!()'
        or (character.isspace() and character != " ")
        for character in value
    ):
        raise ValueError("unsafe command argument")
    return value

def run_cmd(arguments, *, cwd, env):
    values = [command_part(value) for value in arguments]
    directory = command_part(cwd)
    quoted = ['"' + value + "\\" * (len(value) - len(value.rstrip("\\"))) + '"' for value in values]
    command = 'cmd.exe /d /v:off /s /c "' + " ".join(quoted) + '"'
    return subprocess.run(command, cwd=directory, env=env, check=True, shell=False)

parser = argparse.ArgumentParser()
parser.add_argument("--runtime", required=True)
parser.add_argument("--check-only", action="store_true")
args = parser.parse_args()
root = pathlib.Path(__file__).resolve().parents[2]
approved = pathlib.Path(r"C:\Users\porad\AppData\Local\Temp\opencode")
cargo = pathlib.Path.home() / ".cargo/bin/cargo.exe"
for value in (args.runtime, root, approved, cargo, root / "scripts/migration", root / "rust/target/profile-helper-roundtrip"):
    command_part(value)
environment = os.environ.copy()
environment["CARGO_TARGET_DIR"] = str(root / "rust/target/profile-helper-roundtrip")
environment["CARGO_PROFILE_TEST_DEBUG"] = "0"
environment["CARGO_PROFILE_DEV_DEBUG"] = "0"
environment["CARGO_BUILD_JOBS"] = "2"
with tempfile.TemporaryDirectory(prefix="zabor-profile-helper-harness-", dir=approved) as directory:
    directory = pathlib.Path(directory)
    native = (root / "rust/crates/zabor-native").as_posix()
    helper = (root / "rust/crates/zabor-profile-helper").as_posix()
    (directory / "Cargo.toml").write_text(f'''[workspace]
[package]
name = "zabor-profile-helper-harness"
version = "0.1.0"
edition = "2021"
[dependencies]
zabor-native = {{ path = "{native}", default-features = false }}
zabor-profile-helper = {{ path = "{helper}", features = ["test-source"] }}
serde_json = "=1.0.154"
sha2 = "=0.10.9"
p256 = {{ version = "=0.13.2", features = ["jwk"] }}
tempfile = "=3.27.0"
[patch.crates-io]
tract-core = {{ path = "{(root / 'rust/vendor/tract-core-0.21.4').as_posix()}" }}
webrtc-vad = {{ path = "{(root / 'rust/vendor/webrtc-vad-0.4.0').as_posix()}" }}
rnnoise2-sys = {{ path = "{(root / 'rust/vendor/rnnoise2-sys-0.1.2').as_posix()}" }}
''', encoding="utf-8")
    (directory / "src").mkdir()
    shutil.copyfile(root / "scripts/migration/native-profile-roundtrip.rs", directory / "src/main.rs")
    shutil.copyfile(root / "rust/Cargo.lock", directory / "Cargo.lock")
    run_cmd([cargo, "generate-lockfile", "--offline"], cwd=directory, env=environment)
    reference = tomllib.loads((root / "rust/Cargo.lock").read_text(encoding="utf-8"))
    actual = tomllib.loads((directory / "Cargo.lock").read_text(encoding="utf-8"))
    pins = {(item["name"], item["version"], item.get("source"), item.get("checksum")) for item in reference["package"]}
    changed = [item["name"] for item in actual["package"] if item.get("source") and (item["name"], item["version"], item.get("source"), item.get("checksum")) not in pins]
    if changed:
        raise RuntimeError("isolated harness changed existing dependency pins: " + ",".join(changed))
    run_cmd([cargo, "check", "-j2", "--offline", "--locked"], cwd=directory, env=environment)
    if not args.check_only:
        run_cmd([cargo, "run", "-j2", "--offline", "--locked", "--", args.runtime, root / "scripts/migration", approved], cwd=directory, env=environment)
