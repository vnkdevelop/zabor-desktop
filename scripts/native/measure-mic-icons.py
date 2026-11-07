import argparse
import hashlib
import json
import math
import xml.etree.ElementTree as ET
from pathlib import Path

import numpy as np
from PIL import Image


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def icon_pixels(path, scale, metrics=None):
    image = Image.open(path).convert("RGB")
    if metrics is not None:
        x = math.floor(metrics["panelX"] * scale + 0.5)
        y = math.floor(metrics["panelY"] * scale + 0.5)
        image = image.crop((x, y, x + math.floor(metrics["panelWidth"] * scale + 0.5), y + math.ceil(metrics["panelHeight"] * scale)))
    box = (round(8 * scale), round(7 * scale), round(30 * scale), round(29 * scale))
    return image.crop(box), box, image


def difference(left, right):
    a, b = np.asarray(left, dtype=np.int16), np.asarray(right, dtype=np.int16)
    if a.shape != b.shape:
        raise ValueError("Icon pixel dimensions differ")
    error = np.abs(a - b)
    return {"meanChannelError": float(error.mean()), "maxChannelError": int(error.max()), "differentPixels": int(np.any(error != 0, axis=2).sum())}


def ink(image):
    pixels = np.asarray(image.convert("RGB"), dtype=np.float64)
    weights = np.maximum(pixels.mean(axis=2) - 50, 0)
    points = np.argwhere(weights > 80)
    if not len(points) or not weights.sum():
        raise ValueError("Icon ink is missing")
    low, high = points.min(axis=0), points.max(axis=0) + 1
    y, x = np.indices(weights.shape)
    return {"bounds": [int(low[1]), int(low[0]), int(high[1]), int(high[0])], "centroid": [float((x * weights).sum() / weights.sum()), float((y * weights).sum() / weights.sum())]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--captures", required=True)
    parser.add_argument("--previous", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    root, previous, output = Path(args.captures), Path(args.previous), Path(args.output)
    if output.exists():
        raise ValueError("Icon measurement output already exists")
    report_path, prior_path = root / "verification.json", previous / "verification.json"
    report, prior = json.loads(report_path.read_text()), json.loads(prior_path.read_text())
    if report["automatedChecks"] != "passed" or prior["automatedChecks"] != "passed":
        raise ValueError("Panel captures must pass their guards")
    sources = {**report["sources"], str(Path(__file__)): digest(__file__)}
    binary = Path("rust/target/debug/zabor-native.exe")
    if digest(binary) != report["binarySha256"]:
        raise ValueError("Icon capture binary is not current")
    for file, value in sources.items():
        if digest(file) != value:
            raise ValueError("Icon source hash mismatch")
    contours = {}
    native = Path("rust/crates/zabor-native/ui/mic-test.slint").read_text()
    for kind in ["play", "pause"]:
        asset = Path(f"rust/crates/zabor-native/assets/icons/test-{kind}.svg")
        svg = ET.fromstring(asset.read_text())
        path = svg.find("{http://www.w3.org/2000/svg}path")
        contours[kind] = path.attrib["d"]
        if svg.attrib["viewBox"] != "0 0 256 256" or path.attrib["fill"] != "white":
            raise ValueError("Native icon viewbox or color changed")
    binding = 'Image { x: 11px; y: 11px; width: 14px; height: 14px; source: root.test.playing ? @image-url("../assets/icons/test-pause.svg") : @image-url("../assets/icons/test-play.svg"); }'
    if native.count(binding) != 1:
        raise ValueError("Native icon asset, state, position or size binding changed")
    play_asset = "rust/crates/zabor-native/assets/icons/test-play.svg"
    if digest(play_asset) != prior["sources"][play_asset]:
        raise ValueError("Unaccepted play asset must remain unchanged")
    files = {str(report_path): digest(report_path), str(prior_path): digest(prior_path)}
    output.mkdir(parents=True)
    measurements, unexpected_changes, reference_changes = [], [], []
    for current in report["captures"]:
        before = next(item for item in prior["captures"] if (item["locale"], item["scale"], item["state"]) == (current["locale"], current["scale"], current["state"]))
        paths = [(root / current["file"], current["sha256"]), (previous / before["file"], before["sha256"]), (Path(current["reference"]["file"]), current["reference"]["sha256"])]
        for path, value in paths:
            if digest(path) != value:
                raise ValueError("Icon capture image hash mismatch")
            files[str(path)] = value
        if current["reference"]["sha256"] != before["reference"]["sha256"]:
            reference_changes.append(current["file"])
        scale = float(current["scale"])
        ready = current["state"] in ["playing", "paused", "silent"]
        if not ready:
            if current["sha256"] != before["sha256"]:
                unexpected_changes.append(current["file"])
            if current["reference"]["metrics"]["icon"] is not None:
                raise ValueError("Non-ready reference unexpectedly has an icon")
            continue
        kind = "play" if current["state"] == "paused" else "pause"
        icon = current["reference"]["metrics"]["icon"]
        if (icon["x"], icon["y"], icon["width"], icon["height"], icon["viewBox"], icon["fill"]) != (12, 11, 14, 14, "0 0 256 256", "rgb(255, 255, 255)"):
            raise ValueError("Reference icon position, color or size changed")
        contour_matches = icon["paths"] == [contours[kind]]
        if kind == "pause" and not contour_matches:
            raise ValueError("Pause contour differs from actual React SVG")
        now, box, whole = icon_pixels(paths[0][0], scale, current["metrics"])
        old, _, whole_old = icon_pixels(paths[1][0], scale, before["metrics"])
        reference, _, _ = icon_pixels(paths[2][0], scale)
        outside, outside_old = np.asarray(whole).copy(), np.asarray(whole_old).copy()
        outside[box[1]:box[3], box[0]:box[2]] = 0
        outside_old[box[1]:box[3], box[0]:box[2]] = 0
        if not np.array_equal(outside, outside_old):
            unexpected_changes.append(current["file"])
        old_error, new_error = difference(old, reference), difference(now, reference)
        native_ink, reference_ink = ink(now), ink(reference)
        bound_error = max(abs(a - b) for a, b in zip(native_ink["bounds"], reference_ink["bounds"]))
        passed = new_error["meanChannelError"] < old_error["meanChannelError"] and bound_error <= 1 if kind == "pause" else current["sha256"] == before["sha256"]
        measurements.append({"state": current["state"], "locale": current["locale"], "scale": current["scale"], "kind": kind, "check": "pause-improvement" if kind == "pause" else "play-unchanged", "contourMatchesReference": contour_matches, "previous": old_error, "current": new_error, "nativeInk": native_ink, "referenceInk": reference_ink, "maxBoundErrorPhysicalPx": bound_error, "passed": passed})
        panel = Image.new("RGB", (now.width * 3 + 8, now.height * 2 + 4), (6, 6, 6))
        for index, image in enumerate([old, now, reference]):
            panel.paste(image, (index * (now.width + 4), 0))
        error = np.abs(np.asarray(now, dtype=np.int16) - np.asarray(reference, dtype=np.int16)).astype(np.uint8)
        panel.paste(Image.fromarray(error), (now.width + 4, now.height + 4))
        path = output / f"{current['state']}-{current['locale']}-{current['scale']}.png"
        panel.resize((panel.width * 6, panel.height * 6), Image.Resampling.NEAREST).save(path)
        files[str(path)] = digest(path)
    passed = len(measurements) == 24 and all(item["passed"] for item in measurements) and not unexpected_changes and not reference_changes
    for file, value in {**files, **sources}.items():
        if digest(file) != value:
            raise ValueError("Icon evidence changed during measurement")
    if digest(binary) != report["binarySha256"]:
        raise ValueError("Binary changed during icon measurement")
    result = {"iconImprovementChecks": "passed" if passed else "failed", "pauseContourAccepted": passed, "playContourAccepted": False, "binarySha256": report["binarySha256"], "previousBinarySha256": prior["binarySha256"], "sources": sources, "files": files, "measurements": measurements, "unexpectedChanges": unexpected_changes, "referenceChanges": reference_changes, "nativeClientReady": False, "micTestUiReady": False, "focusRasterAccepted": False, "scope": "exact React DOM/native SVG pause contour, unchanged14x14 white fill/placement/state binding; strict lower pause mean-channel pixel error and <=1 physical-pixel ink bounds at16measurements, with8play images/asset and all non-icon/nonready/reference pixels unchanged; play contour mismatch retained/unaccepted; reduced-motion synthetic local-only captures, not exact raster/subpixel/full UI/cold/physical acceptance"}
    (output / "comparison.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps({"iconImprovementChecks": result["iconImprovementChecks"], "measurements": len(measurements), "unexpectedChanges": unexpected_changes, "referenceChanges": reference_changes, "failures": [item for item in measurements if not item["passed"]]}))
    if not passed:
        raise ValueError("Icon pixel improvement or isolation checks failed")


if __name__ == "__main__":
    main()
