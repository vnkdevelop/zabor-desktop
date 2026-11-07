import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
from PIL import Image


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def color_mask(image):
    pixels = np.asarray(image.convert("RGB"), dtype=np.int16)
    return np.max(np.abs(pixels - np.array([238, 104, 169])), axis=2) <= 24


def outline(image, before, box=None, halo=4):
    if image.size != before.size:
        raise ValueError("Focus capture size changed within a state")
    mask = color_mask(image) & ~color_mask(before)
    if box is not None:
        left, top, right, bottom = box
        region = np.zeros(mask.shape, dtype=bool)
        region[max(0, int(np.floor(top - halo))):int(np.ceil(bottom + halo)), max(0, int(np.floor(left - halo))):int(np.ceil(right + halo))] = True
        mask &= region
        mask[max(0, int(np.floor(top))):int(np.ceil(bottom)), max(0, int(np.floor(left))):int(np.ceil(right))] = False
    points = np.argwhere(mask)
    if not len(points):
        return None, mask
    low, high = points.min(axis=0), points.max(axis=0) + 1
    return [int(low[1]), int(low[0]), int(high[1]), int(high[0])], mask


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--captures", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    captures, output = Path(args.captures), Path(args.output)
    if output.exists():
        raise ValueError("Focus measurement output already exists")
    report = json.loads((captures / "verification.json").read_text())
    if report["automatedChecks"] != "passed":
        raise ValueError("Focus semantics must pass before outline measurements")
    sources = dict(report["sources"])
    sources[str(Path(__file__))] = digest(__file__)
    binary = Path("rust/target/debug/zabor-native.exe")
    if digest(binary) != report["binarySha256"]:
        raise ValueError("Focus capture binary is not current")
    for file, value in sources.items():
        if digest(file) != value:
            raise ValueError("Focus source hash mismatch")
    files, measurements = {}, []
    output.mkdir(parents=True)
    for item in report["captures"]:
        if item["metrics"]["hidden"]:
            continue
        action = item["action"]
        base_action = "fresh-initial" if action.startswith("fresh-") else "initial"
        base = next(candidate for candidate in report["captures"] if candidate["locale"] == item["locale"] and candidate["scale"] == item["scale"] and candidate["state"] == item["state"] and candidate["action"] == base_action)
        native_path, native_base = captures / item["file"], captures / base["file"]
        reference_path = captures / "react" / item["reference"]["file"]
        reference_base = captures / "react" / base["reference"]["file"]
        for path, expected in [(native_path, item["sha256"]), (native_base, base["sha256"]), (reference_path, item["reference"]["sha256"]), (reference_base, base["reference"]["sha256"])]:
            if digest(path) != expected:
                raise ValueError("Focus evidence image hash mismatch")
            files[str(path)] = expected
        native = Image.open(native_path).convert("RGB")
        reference = Image.open(reference_path).convert("RGB")
        metrics = item["reference"]["metrics"]
        control = next((control for control in metrics["controls"] if control["kind"] == metrics["focusedControl"]), None)
        scale = float(item["scale"])
        reference_box = native_box = None
        if control:
            crop = metrics["captureCrop"]
            x = metrics["panelX"] * scale - crop["x"]
            y = metrics["panelY"] * scale - crop["y"]
            reference_box = (x + control["x"] * scale, y + control["y"] * scale, x + (control["x"] + control["width"]) * scale, y + (control["y"] + control["height"]) * scale)
            x = item["metrics"]["panelX"] * scale - np.floor((item["metrics"]["panelX"] - 6) * scale + 0.5)
            y = item["metrics"]["panelY"] * scale - np.floor((item["metrics"]["panelY"] - 6) * scale + 0.5)
            native_box = (x + control["x"] * scale, y + control["y"] * scale, x + (control["x"] + control["width"]) * scale, y + (control["y"] + control["height"]) * scale)
        native_bounds, native_mask = outline(native, Image.open(native_base), native_box, 4 * scale + 1)
        reference_bounds, reference_mask = outline(reference, Image.open(reference_base), reference_box, 4 * scale + 1)
        focused = item["reference"]["metrics"]["focusVisible"] and bool(item["reference"]["metrics"]["focusedControl"])
        if not focused:
            if action in ["initial", "fresh-initial", "fresh-pointer-down", "fresh-pointer-cancel", "fresh-pointer-control", "fresh-pointer-alt", "pointer-playback-down", "pointer-playback-up", "pointer-retake-down", "pointer-retake-cancel"]:
                measurements.append({"locale": item["locale"], "scale": item["scale"], "state": item["state"], "action": action, "focused": False, "nativeBounds": native_bounds, "referenceBounds": reference_bounds, "passed": native_bounds is None and reference_bounds is None})
            continue
        if action not in ["tab-first", "tab-seek", "tab-retake", "reverse-seek", "pointer-to-keyboard", "reverse-from-pointer", "reverse-playback", "fresh-pointer-shift"]:
            continue
        error = max(abs(left - right) for left, right in zip(native_bounds, reference_bounds)) if native_bounds and reference_bounds else None
        measurements.append({"locale": item["locale"], "scale": item["scale"], "state": item["state"], "action": action, "focused": True, "control": item["metrics"]["focusedControl"], "nativeBounds": native_bounds, "referenceBounds": reference_bounds, "maxBoundErrorPhysicalPx": error, "passed": error is not None and error <= 1})
        if action in ["tab-first", "tab-seek", "tab-retake", "fresh-pointer-shift"]:
            pair = Image.new("RGB", (max(native.width, reference.width) * 2 + 8, max(native.height, reference.height) * 2 + 8), (6, 6, 6))
            pair.paste(native, (0, 0))
            pair.paste(reference, (native.width + 8, 0))
            pair.paste(Image.fromarray(native_mask.astype(np.uint8) * 255).convert("RGB"), (0, native.height + 8))
            pair.paste(Image.fromarray(reference_mask.astype(np.uint8) * 255).convert("RGB"), (native.width + 8, reference.height + 8))
            detail = output / f"{item['state']}-{action}-{item['locale']}-{item['scale']}.png"
            pair.resize((pair.width * 3, pair.height * 3), Image.Resampling.NEAREST).save(detail)
            files[str(detail)] = digest(detail)
    passed = all(item["passed"] for item in measurements)
    for file, value in {**sources, **files}.items():
        if digest(file) != value:
            raise ValueError("Focus evidence changed during measurements")
    if digest(binary) != report["binarySha256"]:
        raise ValueError("Binary changed during outline measurements")
    result = {"outlineBoundsChecks": "passed" if passed else "failed", "tolerancePhysicalPx": 1, "binarySha256": report["binarySha256"], "sources": sources, "files": files, "measurements": measurements, "nativeClientReady": False, "micTestUiReady": False, "focusRasterAccepted": False, "scope": "thresholded new primaryText-color pixels in the selected control's 4px outline annulus (plus one physical-pixel antialias margin), excluding its body and unrelated moving thumb/glyph pixels; rendered outline bounds within one physical pixel and no-outline pointer checks; not global pixel/antialias/full-modal/accessibility acceptance"}
    (output / "comparison.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps({"outlineBoundsChecks": result["outlineBoundsChecks"], "measurements": len(measurements), "failures": [item for item in measurements if not item["passed"]]}))
    if not passed:
        raise ValueError("Rendered focus bounds or no-outline checks failed")


if __name__ == "__main__":
    main()
