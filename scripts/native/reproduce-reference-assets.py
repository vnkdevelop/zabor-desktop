import base64
import gzip
import importlib.util
import json
from pathlib import Path
import tempfile


SPEC = importlib.util.spec_from_file_location("assets", Path(__file__).with_name("prepare-reference-assets.py"))
ASSETS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ASSETS)


def main():
    checked = ASSETS.check()
    original = ASSETS.CLOSURE.OUTPUT
    data = json.loads((original / "capture-inputs.json").read_bytes())
    sources = json.loads(gzip.decompress((original / "capture-sources.json.gz").read_bytes()))
    base = Path.home() / "AppData/Local/Temp/opencode"
    with tempfile.TemporaryDirectory(prefix="native-reference-reproduce-", dir=base) as temporary:
        root = Path(temporary)
        retained, output = root / "retained", root / "output"
        retained.mkdir()
        output.mkdir()
        for name, encoded in sources.items():
            target = retained / name
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as stream:
                stream.write(base64.b64decode(encoded, validate=True))
        for name in ("closure.json", "tailwind-sources.json.gz"):
            with (output / name).open("xb") as stream:
                stream.write((original / name).read_bytes())
        ASSETS.CLOSURE.OUTPUT = output
        try:
            result = ASSETS.prepare(retained)
        finally:
            ASSETS.CLOSURE.OUTPUT = original
        for row in data["artifacts"]:
            raw = (output / row["path"]).read_bytes()
            raw = gzip.decompress(raw) if row["encoding"] == "gzip" else raw
            if len(raw) != row["size"] or ASSETS.CLOSURE.sha256(raw) != row["sha256"]:
                raise ValueError("Independent frozen CSS/JS reproduction differs")
        print(json.dumps({"reproduction": "exact-byte-match", "sources": checked["sources"],
                          "artifacts": 2, "compiler": result["compiler"], "activeRendererUsed": False,
                          "electronStarted": False, "captureAccepted": False}, indent=2))


if __name__ == "__main__":
    main()
