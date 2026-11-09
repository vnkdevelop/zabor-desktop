import argparse
import ast
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("update_io_harness", Path(__file__).with_name("verify-update-io.py"))
IO = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(IO)


def followup_inheritance(hashes, env):
    prior = ROOT / "migration-artifacts/native-data/update-host-2026-11-08"
    frozen = json.loads((prior / "checked-v5/source-hashes.json").read_text(encoding="utf-8"))
    affected_paths = {
        "rust/crates/zabor-native/src/update_helper.rs", "rust/crates/zabor-native/src/update_host_process.rs",
        "rust/crates/zabor-native/update-helper/update_host_native.cpp", "rust/crates/zabor-native/update-helper/update_host_fixture.cpp",
        "scripts/native/packaging/package.py", "scripts/native/packaging/test_package.py", "scripts/native/packaging/verify-update-host.py"}
    for name, value in frozen.items():
        if name not in affected_paths and hashes[name] != value:
            raise ValueError("Unaffected inherited source changed: " + name)

    def source(name, old):
        if not old:
            return (ROOT / name).read_text(encoding="utf-8")
        return IO.run_line(IO.quoted(["git", "show", "41e0fec:" + name]), ROOT, env, timeout=None).replace("\r\n", "\n")

    def rust_function(text, name):
        match = re.search(r"\bfn " + re.escape(name) + r"\(", text)
        if not match:
            raise ValueError("Missing inherited Rust test: " + name)
        start = text.index("{", match.start())
        depth = 1
        end = start + 1
        while depth:
            if text[end] == "{":
                depth += 1
            elif text[end] == "}":
                depth -= 1
            end += 1
        return text[match.start():end]

    completion = json.loads((prior / "completion.json").read_text(encoding="utf-8"))
    affected_rust = ["update_host::tests::native_health_commits_but_only_drain_observes_original_helper_target_and_children",
                     "update_host::tests::wrong_nonce_early_exit_and_timeout_reap_children_then_recover_previous_selector"]
    guards = {}
    for test in completion["rustTests"]:
        path = "rust/crates/zabor-native/src/" + ("update_host_process.rs" if "::process::" in test else "update_host_tests.rs")
        name = test.rsplit("::", 1)[1]
        old = rust_function(source(path, True), name)
        current = rust_function(source(path, False), name)
        if old != current:
            raise ValueError("Prior Rust test definition changed: " + test)
        guards[test] = hashlib.sha256(current.encode()).hexdigest()
    package_path = "scripts/native/packaging/test_package.py"
    python_tests = ["test_native_helper_is_exactly_allowlisted_and_hashed", "test_non_fixture_requires_original_native_helper",
                    "test_unsigned_helper_or_missing_pin_cannot_enter_certified_route", "test_managed_or_dynamic_crt_helper_is_rejected"]
    old_ast = ast.parse(source(package_path, True))
    current_ast = ast.parse(source(package_path, False))
    functions = lambda tree: {node.name: ast.dump(node, include_attributes=False) for node in ast.walk(tree) if isinstance(node, ast.FunctionDef)}
    old_functions, current_functions = functions(old_ast), functions(current_ast)
    for name in python_tests[:2]:
        if old_functions[name] != current_functions[name]:
            raise ValueError("Unaffected packaging test changed: " + name)
        guards["PackageTests." + name] = hashlib.sha256(current_functions[name].encode()).hexdigest()
    return {"priorUniqueTests": 13, "affectedPriorRust": affected_rust,
            "affectedPriorPackaging": ["PackageTests." + name for name in python_tests[2:]],
            "inheritedPriorRust": [name for name in completion["rustTests"] if name not in affected_rust],
            "inheritedPriorPackaging": ["PackageTests." + name for name in python_tests[:2]],
            "testDefinitionHashes": guards,
            "unaffectedSourceHashes": {name: value for name, value in frozen.items() if name not in affected_paths}}


def main():
    parser = argparse.ArgumentParser(description="Owned synthetic native update host verification")
    parser.add_argument("--attempt", type=int, default=1)
    parser.add_argument("--test-filter", default="update_host::")
    parser.add_argument("--followup", action="store_true")
    args = parser.parse_args()
    if not 1 <= args.attempt <= 100:
        raise ValueError("Invalid evidence attempt")
    if not args.test_filter.startswith("update_host::") or not all(byte.isalnum() or byte in "_:" for byte in args.test_filter):
        raise ValueError("Host-only test filter required")
    parent = Path.home() / "AppData/Local/Temp/opencode"
    source = ROOT / "rust/crates/zabor-native/src"
    cpp_root = ROOT / "rust/crates/zabor-native/update-helper"
    files = [source / name for name in ["release.rs", "update.rs", "update_helper.rs", "update_helper_tests.rs",
             "update_host.rs", "update_host_process.rs", "update_bootstrap.rs", "update_host_tests.rs"]]
    frozen = [*files, *[cpp_root / name for name in ["native_update_helper.cpp", "update_host_native.cpp", "update_host_fixture.cpp"]],
              ROOT / "rust/Cargo.lock", ROOT / "rust/Cargo.toml", ROOT / "rust/crates/zabor-native/Cargo.toml",
              source / "lib.rs", Path(__file__).resolve(), *[Path(__file__).with_name(name) for name in
              ["package.py", "policy.json", "test_package.py"]]]
    hashes = {path.relative_to(ROOT).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest() for path in frozen}
    lock = (ROOT / "rust/Cargo.lock").read_bytes()
    packages = tomllib.loads(lock.decode())["package"]
    wanted = {"fs2": "0.4.", "getrandom": "0.3.", "p256": "0.13.", "semver": "1.", "serde": "1.",
              "serde_json": "1.", "sha2": "0.10.", "tempfile": "3.", "tokio": "1.", "tokio-util": "0.7.",
              "windows-dpapi": "0.2.", "zeroize": "1.", "zip": "9.", "flate2": "1."}
    selected = {}
    for name, prefix in wanted.items():
        matches = [item for item in packages if item["name"] == name and item["version"].startswith(prefix)]
        if len(matches) != 1:
            raise ValueError("Ambiguous frozen dependency: " + name)
        selected[name] = matches[0]["version"]
    permitted = {(item["name"], item["version"], item.get("source")): item.get("checksum") for item in packages}
    env = {name: value for name, value in os.environ.items() if name.upper() in {
        "PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "APPDATA",
        "LOCALAPPDATA", "TEMP", "TMP", "PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432", "PROGRAMDATA", "ALLUSERSPROFILE"}}
    evidence = ROOT / (("migration-artifacts/native-data/update-host-2026-11-09/followup-v" if args.followup else
                       "migration-artifacts/native-data/update-host-2026-11-08/checked-v") + str(args.attempt))
    evidence.mkdir(parents=True, exist_ok=False)
    (evidence / "source-hashes.json").write_text(json.dumps(hashes, indent=2) + "\n", encoding="utf-8")
    inheritance = followup_inheritance(hashes, env) if args.followup else None
    if inheritance:
        (evidence / "inheritance.json").write_text(json.dumps(inheritance, indent=2) + "\n", encoding="utf-8")
    checks = {}
    cargo = Path.home() / ".cargo/bin/cargo.exe"
    vswhere = Path("C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe")
    with tempfile.TemporaryDirectory(prefix="native-update-host-check-", dir=parent) as temporary:
        harness = Path(temporary)

        def check(name, line, compiler=False):
            command = ('call "' + str(vcvars) + '" && ' if compiler else "") + line
            result = subprocess.run('cmd.exe /d /v:off /s /c "' + command + '"', cwd=harness, env=env,
                                    capture_output=True)
            text = result.stdout.decode("utf-8", errors="replace") + result.stderr.decode("utf-8", errors="replace")
            (evidence / (name + ".log")).write_text(text, encoding="utf-8")
            checks[name] = {"exitCode": result.returncode, "command": command,
                            "logSha256": hashlib.sha256((evidence / (name + ".log")).read_bytes()).hexdigest()}
            (evidence / "command-results.json").write_text(json.dumps(checks, indent=2) + "\n", encoding="utf-8")
            if result.returncode:
                raise RuntimeError(name + " failed: " + text[-12000:])
            return text

        install = IO.run_line(IO.quoted([vswhere, "-latest", "-prerelease", "-products", "*", "-requires",
                  "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"]), harness, env, timeout=None).strip()
        if not install:
            raise ValueError("MSVC discovery returned no installation")
        vcvars = Path(install) / "VC/Auxiliary/Build/vcvars64.bat"
        if not vcvars.is_file():
            raise ValueError("Discovered MSVC environment unavailable")
        helper = harness / "zabor-native-update-helper.exe"
        check("native-host", IO.quoted(["cl.exe", "/nologo", "/std:c++20", "/EHsc", "/W4", "/WX", "/MT", "/O2",
              cpp_root / "update_host_native.cpp", "/Fe:" + str(helper), "/link", "advapi32.lib", "wintrust.lib", "crypt32.lib", "bcrypt.lib"]), True)
        if args.followup:
            acquisition = harness / "native-host-acquisition.exe"
            check("native-acquisition", IO.quoted(["cl.exe", "/nologo", "/std:c++20", "/EHsc", "/W4", "/WX", "/MT", "/O2",
                  "/DZABOR_HOST_TEST_ACQUISITION", cpp_root / "update_host_native.cpp", "/Fe:" + str(acquisition),
                  "/link", "advapi32.lib", "wintrust.lib", "crypt32.lib", "bcrypt.lib"]), True)
            env["ZABOR_HOST_TEST_ACQUISITION_HELPER"] = str(acquisition)
        for mode in range(5 if args.followup else 4):
            target = harness / ("fixture-" + str(mode) + ".exe")
            check("fixture-" + str(mode), IO.quoted(["cl.exe", "/nologo", "/std:c++20", "/EHsc", "/W4", "/WX", "/MT", "/O2",
                  "/DFIXTURE_MODE=" + str(mode), cpp_root / "update_host_fixture.cpp", "/Fe:" + str(target)]), True)
            env["ZABOR_HOST_TEST_TARGET_" + str(mode)] = str(target)
        sandboxes = harness / "sandboxes"
        sandboxes.mkdir()
        env.update({"CARGO_TARGET_DIR": str(harness / "target"), "CARGO_NET_OFFLINE": "true",
                    "ZABOR_UPDATE_TEST_DIRECTORY": str(sandboxes), "ZABOR_UPDATE_TEST_HELPER": str(helper),
                    "PYTHONDONTWRITEBYTECODE": "1"})
        features = {"serde": ', features = ["derive"]', "zeroize": ', features = ["serde"]',
                    "tokio": ', features = ["rt-multi-thread", "macros", "sync", "time"]',
                    "zip": ', default-features = false, features = ["deflate-flate2"]',
                    "flate2": ', features = ["rust_backend"]'}
        manifest = ['[package]', 'name = "zabor-update-host-harness"', 'version = "0.1.0"', 'edition = "2021"',
                    '[lib]', 'path = "lib.rs"', '[dependencies]']
        manifest += [name + ' = { version = "=' + version + '"' + features.get(name, "") + ' }' for name, version in selected.items()]
        (harness / "Cargo.toml").write_text("\n".join(manifest) + "\n", encoding="utf-8")
        (harness / "Cargo.lock").write_bytes(lock)
        modules = ['#![forbid(unsafe_code)]']
        for name in ["release", "update", "update_helper", "update_host"]:
            modules += ['#[path = "' + (source / (name + ".rs")).as_posix() + '"]', 'pub mod ' + name + ';']
        modules += ['pub fn stage(root: &update_helper::UpdateRoot, context: update_helper::StageContext, candidate: update_helper::ValidatedRelease, archive: &std::path::Path) -> update_helper::Result<()> { root.prepare(context, candidate, archive) }']
        (harness / "lib.rs").write_text("\n".join(modules) + "\n", encoding="utf-8")
        (harness / ".cargo").mkdir()
        (harness / ".cargo/config.toml").write_text('[target.x86_64-pc-windows-msvc]\nrustflags = ["-C", "target-feature=+crt-static"]\n', encoding="utf-8")
        check("metadata", IO.quoted([cargo, "metadata", "--offline", "--format-version", "1"]))
        derived = tomllib.loads((harness / "Cargo.lock").read_text())["package"]
        for item in derived:
            if item["name"] == "zabor-update-host-harness":
                continue
            if permitted.get((item["name"], item["version"], item.get("source"))) != item.get("checksum") or (item["name"], item["version"], item.get("source")) not in permitted:
                raise ValueError("Dependency outside frozen root lock: " + item["name"])
        check("fmt", IO.quoted([cargo.with_name("rustfmt.exe"), "--edition", "2021", "--check", *files]))
        check("clippy", IO.quoted([cargo, "clippy", "--locked", "--offline", "--all-targets", "-j", "2", "--", "-D", "warnings"]), True)
        filters = [("host-regressions", "update_host::process::tests::followup_")]
        if args.followup:
            filters += [("host-health", inheritance["affectedPriorRust"][0]), ("host-rollback", inheritance["affectedPriorRust"][1])]
        else:
            filters = [("host-tests", args.test_filter)]
        tests = "\n".join(check(name, IO.quoted([cargo, "test", "--locked", "--offline", "-j", "2", selected_filter,
                         "--", "--test-threads=1"]), True) for name, selected_filter in filters)
        package_tests = ["PackageTests.test_unsigned_helper_or_missing_pin_cannot_enter_certified_route",
                         "PackageTests.test_managed_or_dynamic_crt_helper_is_rejected"]
        package_tests += (["PackageTests.test_followup_source_flip_cannot_certify_staged_non_system_imports",
                          "PackageTests.test_followup_staged_bytes_and_namespace_are_held_during_certificate_check",
                          "PackageTests.test_followup_changed_certified_helper_cannot_enter_composition",
                          "PackageTests.test_followup_composition_keeps_certified_helper_bytes_held"] if args.followup else
                         ["PackageTests.test_native_helper_is_exactly_allowlisted_and_hashed", "PackageTests.test_non_fixture_requires_original_native_helper"])
        check("package-tests", IO.quoted(["python", "-B", ROOT / "scripts/native/packaging/test_package.py", *package_tests]))
        for name, expected in hashes.items():
            if hashlib.sha256((ROOT / name).read_bytes()).hexdigest() != expected:
                raise ValueError("Source changed during verification: " + name)
        (evidence / "verification.json").write_text(json.dumps({"sourceHashes": hashes, "frozenDependencies": len(derived) - 1,
            "staticCrtW4WX": True, "fmt": "passed", "clippyAllTargets": "passed", "tests": tests[-5000:],
            "checks": checks, "inheritance": inheritance, "packagingTestsExecuted": package_tests,
            "liveTrust": False, "realAppDrain": False, "legalApproval": False, "nativeClientReady": False,
            "ownedTemporaryWorkspace": temporary}, indent=2) + "\n", encoding="utf-8")
    (evidence / "cleanup.json").write_text(json.dumps({"originalCommandChecksCompleted": True,
        "ownedTemporaryWorkspaceRemoved": not Path(temporary).exists()}, indent=2) + "\n", encoding="utf-8")
    print("Owned harness, native fixture processes and temporary targets cleaned after observed checks.")
    print(tests[-5000:])


if __name__ == "__main__":
    main()
