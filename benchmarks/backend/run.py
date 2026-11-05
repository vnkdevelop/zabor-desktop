import argparse
import base64
import ctypes
import hashlib
import json
import math
import os
import platform
import queue
import secrets
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from pathlib import Path
from datetime import datetime, timezone


CASES = {
    "auth-current": (20, 1, 1000),
    "auth-rejected": (20, 1, 1000),
    "auth-legacy-pbkdf2": (20, 1, 1000),
    "auth-legacy-sha256": (500, 10000, 1000),
    "registry-lookup": (200, 100000, 1000),
    "registry-churn": (100, 10000, 1000),
    "registry-expire": (1000, 1, 1000),
}
SOURCES = ["src/Zabor.Core/Security/EncryptionService.cs", "src/Zabor.Server/Services/CallSessionRegistry.cs"]


def source_hashes(root):
    return {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in SOURCES}


def percentile(values, percent):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * percent) - 1)]


class WindowsMetrics:
    class Counters(ctypes.Structure):
        _fields_ = [("cb", ctypes.c_ulong), ("page_faults", ctypes.c_ulong)] + [
            (name, ctypes.c_size_t) for name in ["peak_working_set", "working_set", "peak_paged_pool", "paged_pool", "peak_nonpaged_pool", "nonpaged_pool", "pagefile", "peak_pagefile", "private_bytes"]
        ]

    def __init__(self, pid):
        from ctypes import wintypes
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.psapi = ctypes.WinDLL("psapi", use_last_error=True)
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
        self.psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(self.Counters), wintypes.DWORD]
        self.handle = self.kernel.OpenProcess(0x410, False, pid)
        if not self.handle:
            raise RuntimeError("cannot inspect own benchmark child")
        self.filetime = wintypes.FILETIME

    def sample(self):
        values = [self.filetime() for _ in range(4)]
        if not self.kernel.GetProcessTimes(self.handle, *[ctypes.byref(value) for value in values]):
            raise RuntimeError("process CPU sample failed")
        counters = self.Counters()
        counters.cb = ctypes.sizeof(counters)
        if not self.psapi.GetProcessMemoryInfo(self.handle, ctypes.byref(counters), counters.cb):
            raise RuntimeError("process memory sample failed")
        ticks = sum((value.dwHighDateTime << 32) | value.dwLowDateTime for value in values[2:])
        return {"cpuMs": ticks / 10000, "workingSetBytes": counters.working_set, "peakWorkingSetBytes": counters.peak_working_set, "privateBytes": counters.private_bytes}

    def close(self):
        self.kernel.CloseHandle(self.handle)


class LinuxMetrics:
    def __init__(self, pid):
        self.root = Path("/proc") / str(pid)

    def sample(self):
        stat = (self.root / "stat").read_text().rsplit(")", 1)[1].split()
        status = {}
        for line in (self.root / "status").read_text().splitlines():
            if line.startswith(("VmRSS:", "VmHWM:")):
                name, value = line.split(":", 1)
                status[name] = int(value.split()[0]) * 1024
        private = 0
        for line in (self.root / "smaps_rollup").read_text().splitlines():
            if line.startswith(("Private_Clean:", "Private_Dirty:", "Private_Hugetlb:")):
                private += int(line.split()[1]) * 1024
        return {"cpuMs": (int(stat[11]) + int(stat[12])) * 1000 / os.sysconf("SC_CLK_TCK"), "workingSetBytes": status["VmRSS"], "peakWorkingSetBytes": status["VmHWM"], "privateBytes": private}

    def close(self):
        pass


def worker(command, payload):
    started = time.perf_counter()
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", env={**os.environ, "ZABOR_ACCEPT_LEGACY_HASHES": "true"})
    messages = queue.Queue(maxsize=4)

    def consume():
        while True:
            line = process.stdout.readline(65537)
            if not line:
                messages.put(None)
                return
            messages.put(line)

    threading.Thread(target=consume, daemon=True).start()
    metrics = None

    def receive():
        line = messages.get(timeout=60)
        if line is None or len(line) > 65536:
            raise RuntimeError("benchmark child did not return a valid record")
        return json.loads(line)

    def send(value):
        process.stdin.write(value + "\n")
        process.stdin.flush()

    try:
        send(json.dumps(payload, separators=(",", ":")))
        ready = receive()
        ready_ms = (time.perf_counter() - started) * 1000
        if ready.get("ready") is not True:
            raise RuntimeError("benchmark child not ready")
        metrics = WindowsMetrics(process.pid) if os.name == "nt" else LinuxMetrics(process.pid)
        send("run")
        if receive().get("warmed") is not True:
            raise RuntimeError("warmup failed")
        before = metrics.sample()
        send("measure")
        result = receive()
        after = metrics.sample()
        expected = payload["iterations"] * payload["batchSize"]
        if payload["operation"] == "registry-expire":
            expected *= payload["residents"]
        if payload["operation"] == "auth-rejected":
            expected = 0
        expected_legacy = expected if payload["operation"].startswith("auth-legacy-") else 0
        samples = result.get("batchMs", [])
        if result.get("successes") != expected or result.get("legacyResults") != expected_legacy or len(samples) != payload["iterations"]:
            raise RuntimeError("semantic parity failed")
        if any(not math.isfinite(value) or value < 0 for value in samples):
            raise RuntimeError("invalid timing sample")
        send("quit")
        if process.wait(timeout=5) != 0:
            raise RuntimeError("benchmark child failed")
        cpu_ms = after["cpuMs"] - before["cpuMs"]
        return {**result, "fixtureProof": ready["fixtureProof"], "readyIncludingFixtureMs": ready_ms, "cpuMeasuredPhaseMs": cpu_ms, "cpuMeasurementReliable": sum(samples) >= 500 and cpu_ms >= 125, "workingSetBytes": after["workingSetBytes"], "peakWorkingSetBytes": after["peakWorkingSetBytes"], "privateBytes": after["privateBytes"]}
    finally:
        if metrics:
            metrics.close()
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        for stream in [process.stdin, process.stdout, process.stderr]:
            stream.close()


def build(root, temporary, backend):
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    cargo = cargo_home / "bin" / ("cargo.exe" if os.name == "nt" else "cargo")
    if not cargo.exists():
        cargo = Path(shutil.which("cargo") or "cargo")
    dotnet = shutil.which("dotnet")
    if not dotnet:
        raise RuntimeError("dotnet SDK required")
    intermediate = (temporary / "dotnet-obj").as_posix() + "/"
    subprocess.run([dotnet, "build", str(root / "dotnet/BackendBench.csproj"), "-c", "Release", "-m:1", "--disable-build-servers", "-o", str(temporary / "dotnet"), f"-p:ZaborBackendRoot={backend.as_posix()}", f"-p:BaseIntermediateOutputPath={intermediate}", f"-p:MSBuildProjectExtensionsPath={intermediate}"], check=True)
    subprocess.run([str(cargo), "fmt", "--manifest-path", str(root / "rust/Cargo.toml"), "--", "--check"], check=True)
    subprocess.run([str(cargo), "clippy", "--manifest-path", str(root / "rust/Cargo.toml"), "--locked", "--target-dir", str(temporary / "rust"), "--", "-D", "warnings"], check=True)
    subprocess.run([str(cargo), "build", "--manifest-path", str(root / "rust/Cargo.toml"), "--release", "--locked", "--target-dir", str(temporary / "rust")], check=True)
    binary = temporary / "rust/release" / ("zabor-backend-bench.exe" if os.name == "nt" else "zabor-backend-bench")
    versions = {"dotnetSdk": subprocess.check_output([dotnet, "--version"], text=True).strip(), "cargo": subprocess.check_output([str(cargo), "--version"], text=True).strip(), "cargoLockSha256": hashlib.sha256((root / "rust/Cargo.lock").read_bytes()).hexdigest(), "rustWorkerBytes": binary.stat().st_size, "dotnetWorkerAssemblyBytes": (temporary / "dotnet/BackendBench.dll").stat().st_size}
    return [dotnet, str(temporary / "dotnet/BackendBench.dll")], [str(binary)], versions


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--backend-root", type=Path, required=True)
    parser.add_argument("--idle-confirmed", action="store_true")
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args()
    if not args.idle_confirmed:
        parser.error("workload execution is deferred until the stream has ended; pass --idle-confirmed only after confirmation")
    if not 5 <= args.repeats <= 20:
        parser.error("five to twenty independent repetitions are required")
    if os.name != "nt" and platform.system() != "Linux":
        parser.error("process metrics support Windows and Linux only")
    root = Path(__file__).resolve().parent
    backend = args.backend_root.resolve()
    fingerprint = source_hashes(backend)
    records = []
    output_directory = Path.cwd() / "migration-artifacts"
    output_directory.mkdir(parents=True, exist_ok=True)
    audit = output_directory / f"backend-comparison-{uuid.uuid4().hex}.jsonl"
    audit.write_text(json.dumps({"type": "run", "completed": False, "backendSourceHashes": fingerprint, "repeats": args.repeats}) + "\n", encoding="utf-8")
    approved_temp = Path(os.environ.get("LOCALAPPDATA", tempfile.gettempdir())) / "Temp/opencode" if os.name == "nt" else Path(tempfile.gettempdir())
    approved_temp.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="zabor-backend-bench-", dir=approved_temp) as directory:
        temporary = Path(directory)
        dotnet, rust, versions = build(root, temporary, backend)
        for operation, (iterations, batch, residents) in CASES.items():
            for repeat in range(args.repeats):
                seed = {"password": secrets.token_hex(24), "salt": base64.b64encode(secrets.token_bytes(16)).decode("ascii"), "operation": operation, "iterations": iterations, "batchSize": batch, "residents": residents, "warmups": 3}
                engines = ["dotnet-legacy", "rust-single-owner"]
                if operation.startswith("registry-"):
                    engines.insert(1, "dotnet-single-owner")
                offset = repeat % len(engines)
                order = engines[offset:] + engines[:offset]
                proofs = set()
                for engine in order:
                    print(f"measuring {operation}: repetition {repeat + 1}/{args.repeats}, {engine}", flush=True)
                    result = worker(rust if engine.startswith("rust-") else dotnet, {**seed, "engine": engine})
                    proofs.add(result["fixtureProof"])
                    record = {**result, "repeat": repeat, "batchSize": batch, "residents": residents}
                    records.append(record)
                    with audit.open("a", encoding="utf-8") as file:
                        file.write(json.dumps({"type": "measurement", **record}) + "\n")
                if len(proofs) != 1 or source_hashes(backend) != fingerprint:
                    raise RuntimeError("fixture or backend-source fingerprint changed")
                print(f"completed {operation}: repetition {repeat + 1}/{args.repeats}")
    summary = {}
    for operation in CASES:
        summary[operation] = {}
        for engine in ["dotnet-legacy", "dotnet-single-owner", "rust-single-owner"]:
            selected = [record for record in records if record["operation"] == operation and record["engine"] == engine]
            if not selected:
                continue
            latencies = [value * 1000 / record["batchSize"] for record in selected for value in record["batchMs"]]
            summary[operation][engine] = {"p50BatchMeanOpUs": percentile(latencies, 0.50), "p95BatchMeanOpUs": percentile(latencies, 0.95), "p99BatchMeanOpUs": percentile(latencies, 0.99), "medianCpuMs": statistics.median(record["cpuMeasuredPhaseMs"] for record in selected), "cpuMeasurementReliable": all(record["cpuMeasurementReliable"] for record in selected), "medianPrivateBytes": statistics.median(record["privateBytes"] for record in selected), "medianReadyIncludingFixtureMs": statistics.median(record["readyIncludingFixtureMs"] for record in selected)}
    comparisons = {}
    for operation, engines in summary.items():
        base = engines["dotnet-legacy"]
        candidate = engines["rust-single-owner"]
        comparisons[operation] = {
            name: None if base[metric] == 0 else 100 * (base[metric] - candidate[metric]) / base[metric]
            for name, metric in [("medianLatencyReductionPercent", "p50BatchMeanOpUs"), ("cpuReductionPercent", "medianCpuMs"), ("privateMemoryReductionPercent", "medianPrivateBytes")]
        }
        if not base["cpuMeasurementReliable"] or not candidate["cpuMeasurementReliable"]:
            comparisons[operation]["cpuReductionPercent"] = None
    output = output_directory / "backend-comparison.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        output.rename(output.with_name(f"backend-comparison-previous-{uuid.uuid4().hex}.json"))
    code_hashes = {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in ["run.py", "dotnet/Program.cs", "rust/src/main.rs"]}
    output.write_text(json.dumps({"scope": "backend-kernel experiment, not full server or SFU performance", "measuredAtUtc": datetime.now(timezone.utc).isoformat(), "idleConfirmed": True, "versions": versions, "benchmarkSourceHashes": code_hashes, "latencyDefinition": "percentiles of batch-mean operation latency, not individual request tail latency", "deployment": "framework-dependent .NET worker versus standalone Rust worker; not full application footprint", "os": platform.platform(), "cpu": platform.processor(), "repeats": args.repeats, "backendSourceHashes": fingerprint, "clockNote": "C# registry uses system tick count; Rust candidate uses process-local monotonic elapsed time", "records": records, "summary": summary, "rustVsLegacy": comparisons}, indent=2), encoding="utf-8")
    with audit.open("a", encoding="utf-8") as file:
        file.write(json.dumps({"type": "complete", "completed": True, "summaryPath": str(output)}) + "\n")
    print(f"results: {output}")


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("benchmark-run-failed; no production process or database was used", file=sys.stderr)
        sys.exit(1)
