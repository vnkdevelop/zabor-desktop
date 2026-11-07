import argparse
import hashlib
import json
import math
from pathlib import Path

import numpy as np
from PIL import Image


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def panel(path, metrics, scale):
    image = Image.open(path).convert("RGB")
    x = math.floor(metrics["panelX"] * scale + 0.5)
    y = math.floor(metrics["panelY"] * scale + 0.5)
    width = math.floor(156 * scale + 0.5)
    height = math.ceil(metrics["panelHeight"] * scale)
    return image.crop((x, y, x + width, y + height))


def ink(image, kind):
    pixels = np.asarray(image, dtype=np.float64)
    red, green, blue = pixels[:, :, 0], pixels[:, :, 1], pixels[:, :, 2]
    if kind == "issue":
        weights = np.clip((red - green - 4) / 134, 0, 1)
    elif kind == "retake":
        weights = np.clip((blue - 32) / 132, 0, 1)
        weights *= (np.abs(red - green) < 35) & (blue >= red)
    else:
        weights = np.clip((green - 90) / 165, 0, 1)
        weights *= np.max(pixels, axis=2) - np.min(pixels, axis=2) < 35
    points = np.argwhere(weights > 0.25)
    if not len(points) or weights.sum() == 0:
        raise ValueError(f"No {kind} glyph evidence")
    low, high = points.min(axis=0), points.max(axis=0)
    rows = weights.sum(axis=1)
    return {
        "bounds": [int(low[1]), int(low[0]), int(high[1] + 1), int(high[0] + 1)],
        "centroidY": float(np.dot(np.arange(len(rows)), rows) / rows.sum()),
        "rows": rows.tolist(),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    parser.add_argument("--captures", required=True)
    parser.add_argument("--previous", required=True)
    args = parser.parse_args()
    output, captures, previous = map(Path, (args.output, args.captures, args.previous))
    if output.exists():
        raise ValueError("Baseline measurement output already exists")
    output.mkdir(parents=True)
    prior = json.loads((previous / "verification.json").read_text())
    binary = Path("rust/target/debug/zabor-native.exe")
    binary_sha256 = digest(binary)
    current_report = json.loads((captures / "verification.json").read_text()) if (captures / "verification.json").exists() else None
    if current_report and current_report["binarySha256"] != binary_sha256:
        raise ValueError("Current capture report binary does not match")
    sources = {str(path): digest(path) for path in [Path(__file__), Path("rust/crates/zabor-native/ui/mic-test.slint"), Path("scripts/native/capture-mic-test.cjs")]}
    files = {}
    results = []
    for scale in (1, 1.25, 1.5, 2):
        for locale in ("ru", "en"):
            for state in ("idle", "playing", "silent", "failed"):
                old = next(item for item in prior["captures"] if item["locale"] == locale and item["state"] == state and float(item["scale"]) == scale)
                name = f"{state}-{locale}-{scale:g}"
                path = captures / f"native-{name}.png"
                if current_report:
                    current_capture = next(item for item in current_report["captures"] if item["locale"] == locale and item["state"] == state and float(item["scale"]) == scale)
                    path = captures / current_capture["file"]
                    if digest(path) != current_capture["sha256"]:
                        raise ValueError("Current evidence image hash mismatch")
                log = captures / f"capture-{name}.log"
                metrics = next(json.loads(line)["nativeMicTestCapture"] for line in log.read_text().splitlines() if line.startswith("{") and "nativeMicTestCapture" in line)
                text_layout = next(json.loads(line)["nativeMicTextLayout"] for line in log.read_text().splitlines() if line.startswith("{") and "nativeMicTextLayout" in line)
                if text_layout["scale"] != scale or text_layout["windowScale"] != scale:
                    raise ValueError("Text placement scale does not match the window DPI")
                current = panel(path, metrics, scale)
                before = panel(previous / old["file"], old["metrics"], scale)
                reference = Image.open(old["reference"]["file"]).convert("RGB")
                if digest(previous / old["file"]) != old["sha256"] or digest(old["reference"]["file"]) != old["reference"]["sha256"]:
                    raise ValueError("Prior evidence image hash mismatch")
                for file in (path, log, previous / old["file"], Path(old["reference"]["file"])):
                    files[str(file)] = digest(file)
                regions = [("primary", 0, 36)] if state in ("idle", "failed") else [("retake", 42, 63)]
                if state in ("silent", "failed"):
                    y = 69 if state == "silent" else 42
                    regions.append(("issue", y - 2, y + 13.75))
                for kind, top, bottom in regions:
                    box = (0, math.floor(top * scale), current.width, math.ceil(bottom * scale))
                    crops = [image.crop(box) for image in (before, current, reference)]
                    measured = [ink(image, kind) for image in crops]
                    old_error = (measured[0]["centroidY"] - measured[2]["centroidY"]) / scale
                    new_error = (measured[1]["centroidY"] - measured[2]["centroidY"]) / scale
                    pair = Image.new("RGB", (current.width * 3 + 16, crops[1].height), (6, 6, 6))
                    for index, image in enumerate(crops):
                        pair.paste(image, (index * (current.width + 8), 0))
                    detail = output / f"{name}-{kind}.png"
                    pair.resize((pair.width * 4, pair.height * 4), Image.Resampling.NEAREST).save(detail)
                    files[str(detail)] = digest(detail)
                    results.append({"locale": locale, "state": state, "scale": scale, "kind": kind,
                        "previous": measured[0], "current": measured[1], "reference": measured[2],
                        "previousCentroidErrorPx": old_error, "currentCentroidErrorPx": new_error,
                        "primaryPixelsUnchanged": crops[0].tobytes() == crops[1].tobytes() if kind == "primary" else None})
    if any(item["primaryPixelsUnchanged"] is False for item in results):
        raise ValueError("Unrelated primary button pixels changed")
    if any(digest(file) != value for file, value in sources.items()):
        raise ValueError("Source changed during baseline measurements")
    if any(digest(file) != value for file, value in files.items()):
        raise ValueError("Image evidence changed during baseline measurements")
    if digest(binary) != binary_sha256:
        raise ValueError("Binary changed during baseline measurements")
    text_row_checks = all(abs(item["currentCentroidErrorPx"]) <= 0.1 for item in results if item["kind"] != "primary")
    report = {"scope": "weighted glyph-row measurements; triptychs are previous native/current native/unchanged React; not exact raster, focus, motion, physical or full-client acceptance",
        "nativeClientReady": False, "micTestUiReady": False, "textRowChecks": "passed" if text_row_checks else "failed", "textRowTolerancePx": 0.1,
        "binarySha256": binary_sha256, "sources": sources, "files": files, "measurements": results}
    (output / "comparison.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    summaries = []
    for kind in ("retake", "issue", "primary"):
        items = [item for item in results if item["kind"] == kind]
        summaries.append({"kind": kind, "measurements": len(items),
            "previousMaxCentroidErrorPx": max(abs(item["previousCentroidErrorPx"]) for item in items),
            "currentMaxCentroidErrorPx": max(abs(item["currentCentroidErrorPx"]) for item in items)})
    print(json.dumps({"summaries": summaries, "micTestUiReady": False}))
    if not text_row_checks:
        raise ValueError("Retake/issue glyph rows differ from the reference by more than 0.1px")


if __name__ == "__main__":
    main()
