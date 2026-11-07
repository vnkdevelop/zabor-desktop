import argparse
import hashlib
import json
import math
from pathlib import Path

import numpy as np
from PIL import Image


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def ring(image, metrics, control, scale, native):
    pixels = np.asarray(image.convert("RGB"), dtype=np.int16)
    mask = np.max(np.abs(pixels - np.array([238, 104, 169])), axis=2) <= 24
    crop_x = math.floor((metrics["panelX"] - 6) * scale + 0.5) if native else metrics["captureCrop"]["x"]
    crop_y = math.floor((metrics["panelY"] - 6) * scale + 0.5) if native else metrics["captureCrop"]["y"]
    x = metrics["panelX"] * scale - crop_x
    y = metrics["panelY"] * scale - crop_y
    factor = metrics["scaleX"]
    left = x + (control["x"] + control["width"] * (1 - factor) / 2) * scale
    top = y + (control["y"] + control["height"] * (1 - factor) / 2) * scale
    right = left + control["width"] * factor * scale
    bottom = top + control["height"] * factor * scale
    halo = 4 * scale + 1
    region = np.zeros(mask.shape, dtype=bool)
    region[max(0, math.floor(top - halo)):math.ceil(bottom + halo), max(0, math.floor(left - halo)):math.ceil(right + halo)] = True
    region[max(0, math.floor(top)):math.ceil(bottom), max(0, math.floor(left)):math.ceil(right)] = False
    mask &= region
    points = np.argwhere(mask)
    if not len(points):
        raise ValueError("Pressed outline ink is missing")
    low, high = points.min(axis=0), points.max(axis=0) + 1
    return [int(low[1]), int(low[0]), int(high[1]), int(high[0])], mask


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--captures", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    root, output = Path(args.captures), Path(args.output)
    if output.exists():
        raise ValueError("Pressed measurement output already exists")
    report_path = root / "verification.json"
    report = json.loads(report_path.read_text())
    if report["automatedChecks"] != "passed" or report["pressedStateAccepted"] is not True:
        raise ValueError("Pressed state checks must pass first")
    sources = {**report["sources"], str(Path(__file__)): digest(__file__)}
    files = {str(report_path): digest(report_path), **report["files"]}
    binary = Path("rust/target/debug/zabor-native.exe")
    if digest(binary) != report["binarySha256"]:
        raise ValueError("Pressed binary is not current")
    for file, value in {**sources, **files}.items():
        if digest(file) != value:
            raise ValueError("Pressed source/evidence hash mismatch")
    output.mkdir(parents=True)
    measurements = []
    for item in report["captures"]:
        if item["scenario"] != "space" or item["action"] not in ["space-held", "repeat-down"]:
            continue
        rest = next(row for row in report["captures"] if (row["state"], row["locale"], row["scale"], row["scenario"], row["action"]) == (item["state"], item["locale"], item["scale"], "space", "rest"))
        scale = float(item["scale"])
        control = rest["reference"]["metrics"]["box"]
        paths = [rest["file"], item["file"], rest["reference"]["file"], item["reference"]["file"]]
        images = [Image.open(path).convert("RGB") for path in paths]
        bounds, masks = [], []
        for image, metrics, native in zip(images, [rest["metrics"], item["metrics"], rest["reference"]["metrics"], item["reference"]["metrics"]], [True, True, False, False]):
            bound, mask = ring(image, metrics, control, scale, native)
            bounds.append(bound)
            masks.append(mask)
        error = max(abs(a - b) for a, b in zip(bounds[1], bounds[3]))
        native_contraction = bounds[0][2] - bounds[0][0] - (bounds[1][2] - bounds[1][0])
        reference_contraction = bounds[2][2] - bounds[2][0] - (bounds[3][2] - bounds[3][0])
        primary = item["metrics"]["target"] == "primary"
        unchanged = digest(paths[0]) == digest(paths[1]) and digest(paths[2]) == digest(paths[3])
        passed = error <= 1 and (native_contraction > 0 and reference_contraction > 0 if primary else unchanged and native_contraction == 0 and reference_contraction == 0)
        measurements.append({"locale": item["locale"], "scale": item["scale"], "state": item["state"], "action": item["action"], "target": item["metrics"]["target"], "nativeRestBounds": bounds[0], "nativeHeldBounds": bounds[1], "referenceRestBounds": bounds[2], "referenceHeldBounds": bounds[3], "nativeContractionPhysicalPx": native_contraction, "referenceContractionPhysicalPx": reference_contraction, "retakeImagesUnchanged": unchanged if not primary else None, "maxBoundErrorPhysicalPx": error, "passed": passed})
        panel = Image.new("RGB", (images[0].width * 2 + 8, images[0].height * 2 + 8), (6, 6, 6))
        for index, image in enumerate(images):
            panel.paste(image, ((index % 2) * (image.width + 8), (index // 2) * (image.height + 8)))
        path = output / f"{item['state']}-{item['action']}-{item['locale']}-{item['scale']}.png"
        panel.resize((panel.width * 3, panel.height * 3), Image.Resampling.NEAREST).save(path)
        files[str(path)] = digest(path)
    expected = 8 * len({(item["locale"], item["scale"]) for item in report["captures"]})
    passed = len(measurements) == expected and all(item["passed"] for item in measurements)
    for file, value in {**sources, **files}.items():
        if digest(file) != value:
            raise ValueError("Pressed evidence changed during measurement")
    if digest(binary) != report["binarySha256"]:
        raise ValueError("Pressed executable changed during measurement")
    result = {"pressedOutlineChecks": "passed" if passed else "failed", "binarySha256": report["binarySha256"], "sources": sources, "files": files, "measurements": measurements, "tolerancePhysicalPx": 1, "focusRasterAccepted": False, "nativeClientReady": False, "scope": "actual settled keyboard-held primary annulus bounds contract in both renderers and agree within one physical pixel; retake held/repeat images stay byte-identical to rest in each renderer; selected control 4px annulus plus one physical-pixel AA margin, not exact global raster/full-modal/continuous-input acceptance"}
    (output / "comparison.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps({"pressedOutlineChecks": result["pressedOutlineChecks"], "measurements": len(measurements), "failures": [item for item in measurements if not item["passed"]]}))
    if not passed:
        raise ValueError("Pressed outline contraction or retake immobility failed")


if __name__ == "__main__":
    main()
