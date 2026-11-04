import argparse
import hashlib
import importlib.util
import json
import sys
import urllib.request
import zipfile
from pathlib import Path

import numpy as np

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("zabor_audio_authoring", ROOT / "scripts" / "generate-ui-sounds.py")
AUTHORING = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUTHORING)
RATE = 48000
SOURCE_DIR = ROOT / "scripts" / "ui-sound-sources" / "memes"
OUTPUT_DIR = ROOT / "src" / "renderer" / "src" / "assets" / "sounds" / "candidates" / "memes"
SOUNDS = [
    {"name": "mute", "sample": "bonk", "title": "bonk doge", "page": "https://www.myinstants.com/en/instant/bonk-doge-81934/", "url": "https://www.myinstants.com/media/sounds/bonk_7zPAD7C.mp3", "max_duration": 0.38, "rms_dbfs": -26, "peak_dbfs": -11.5, "filters": "highpass=f=65,lowpass=f=10500"},
    {"name": "unmute", "sample": "pop", "title": "Pop SFX", "page": "https://www.myinstants.com/en/instant/pop-sfx-75405/", "url": "https://www.myinstants.com/media/sounds/pop_7e9Is8L.mp3", "max_duration": 0.28, "rms_dbfs": -26, "peak_dbfs": -11.5, "filters": "highpass=f=85,lowpass=f=9500"},
    {"name": "stream-start", "sample": "vine-boom", "title": "VINE BOOM SOUND", "page": "https://www.myinstants.com/en/instant/vine-boom-sound-70972/", "url": "https://www.myinstants.com/media/sounds/vine-boom.mp3", "max_duration": 1.0, "rms_dbfs": -25.5, "peak_dbfs": -10.5, "filters": "highpass=f=45,lowpass=f=9500,equalizer=f=80:t=q:w=0.8:g=-2"},
    {"name": "stream-stop", "sample": "bruh", "title": "BRUH", "page": "https://www.myinstants.com/en/instant/bruh/", "url": "https://www.myinstants.com/media/sounds/movie_1.mp3", "max_duration": 0.75, "rms_dbfs": -26, "peak_dbfs": -12, "filters": "highpass=f=85,lowpass=f=8500"},
    {"name": "message", "sample": "quack", "title": "Single Quack Meme", "page": "https://www.myinstants.com/en/instant/single-quack-meme-12976/", "url": "https://www.myinstants.com/media/sounds/single-quack-meme.mp3", "max_duration": 0.38, "rms_dbfs": -31.5, "peak_dbfs": -18.5, "filters": "highpass=f=120,lowpass=f=7500,equalizer=f=3000:t=q:w=0.9:g=-2"},
]


def fetch_sources():
    SOURCE_DIR.mkdir(parents=True, exist_ok=True)
    report = []
    for sound in SOUNDS:
        path = SOURCE_DIR / f"{sound['sample']}.mp3"
        if not path.exists():
            request = urllib.request.Request(sound["url"], headers={"User-Agent": "Mozilla/5.0", "Referer": sound["page"]})
            with urllib.request.urlopen(request, timeout=45) as response:
                data = response.read(5 * 1024 * 1024 + 1)
            if len(data) > 5 * 1024 * 1024:
                raise ValueError("Source sample exceeds the download size limit")
            path.write_bytes(data)
        audio = AUTHORING.decode(path, rate=RATE, channels=2)
        if not len(audio) or not np.isfinite(audio).all() or float(np.max(np.abs(audio))) < 1e-8:
            raise ValueError(f"Invalid source sample: {path.name}")
        report.append({"sample": sound["sample"], "duration_s": round(len(audio) / RATE, 4), "source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "license": "unverified; audition only"})
    return report


def prepare(sound):
    path = SOURCE_DIR / f"{sound['sample']}.mp3"
    audio = AUTHORING.decode(path, rate=RATE, channels=2)
    audio = AUTHORING.process(audio, sound["filters"], channels=2, input_rate=RATE, output_rate=RATE)
    activity = np.flatnonzero(np.max(np.abs(audio), axis=1) > np.max(np.abs(audio)) * 10 ** (-42 / 20))
    if not len(activity):
        raise ValueError("Processed sample is silent")
    start = max(0, int(activity[0]) - round(0.002 * RATE))
    end = min(len(audio), int(activity[-1]) + round(0.025 * RATE))
    audio = audio[start:min(end, start + round(sound["max_duration"] * RATE))]
    audio = AUTHORING.fade(audio, attack=0.0015, release=0.055 if sound["name"] == "stream-start" else 0.012, rate=RATE)
    rms = float(np.sqrt(np.mean(audio ** 2)))
    peak = float(np.max(np.abs(audio)))
    gain = min(10 ** (sound["rms_dbfs"] / 20) / rms, 10 ** (sound["peak_dbfs"] / 20) / peak)
    return audio * gain


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fetch-only", action="store_true")
    parser.add_argument("--overwrite", action="store_true")
    arguments = parser.parse_args()
    sources = fetch_sources()
    if arguments.fetch_only:
        print(json.dumps(sources, indent=2))
        return
    paths = [OUTPUT_DIR / f"{sound['name']}.{extension}" for sound in SOUNDS for extension in ["wav", "mp3"]]
    paths.extend([OUTPUT_DIR / "preview.wav", OUTPUT_DIR / "preview.mp3", OUTPUT_DIR / "manifest.json", OUTPUT_DIR / "zabor-meme-ui-sounds.zip"])
    if not arguments.overwrite and any(path.exists() for path in paths):
        raise FileExistsError("Meme candidates already exist; --overwrite is required to replace them")
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    manifest = {
        "revision": 2,
        "style": "recognizable meme samples; minimal editing, no synthesized tone layers",
        "runtime_integration": False,
        "redistribution_cleared": False,
        "rights_status": "Source availability does not grant release or redistribution permission. Audition candidates only.",
        "sources": sources,
        "sounds": {},
        "preview_sections": [],
    }
    parts = [np.zeros((round(0.25 * RATE), 2))]
    cursor = 0.25
    for sound in SOUNDS:
        audio = prepare(sound)
        wav = OUTPUT_DIR / f"{sound['name']}.wav"
        mp3 = OUTPUT_DIR / f"{sound['name']}.mp3"
        AUTHORING.encode(wav, audio, ["-c:a", "pcm_s24le"])
        AUTHORING.encode(mp3, audio, ["-ar", "44100", "-c:a", "libmp3lame", "-b:a", "192k"])
        manifest["sounds"][sound["name"]] = {
            "sample": sound["title"],
            "source_page": sound["page"],
            "source_url": sound["url"],
            "license": "unverified",
            "filters": sound["filters"],
            "wav": AUTHORING.measure(wav),
            "mp3": AUTHORING.measure(mp3),
        }
        manifest["preview_sections"].append({"name": sound["name"], "sample": sound["title"], "start_s": round(cursor, 4), "repetitions": 2})
        for _ in range(2):
            parts.extend([audio, np.zeros((round(0.6 * RATE), 2))])
            cursor += len(audio) / RATE + 0.6
        parts.append(np.zeros((round(0.45 * RATE), 2)))
        cursor += 0.45
    preview = np.concatenate(parts)
    AUTHORING.encode(OUTPUT_DIR / "preview.wav", preview, ["-c:a", "pcm_s24le"])
    AUTHORING.encode(OUTPUT_DIR / "preview.mp3", preview, ["-ar", "44100", "-c:a", "libmp3lame", "-b:a", "192k"])
    manifest["preview"] = AUTHORING.measure(OUTPUT_DIR / "preview.mp3")
    (OUTPUT_DIR / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    readme = OUTPUT_DIR / "README.md"
    if not readme.exists():
        raise FileNotFoundError("The audition rights notice must exist before creating the archive")
    with zipfile.ZipFile(OUTPUT_DIR / "zabor-meme-ui-sounds.zip", "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in paths:
            if path.suffix != ".zip" and path.name != "preview.wav":
                archive.write(path, arcname=path.name)
        archive.write(readme, arcname="README.md")
    print(json.dumps({"output": str(OUTPUT_DIR), "sounds": manifest["sounds"], "preview_sections": manifest["preview_sections"], "preview": manifest["preview"]}, indent=2))


if __name__ == "__main__":
    main()
