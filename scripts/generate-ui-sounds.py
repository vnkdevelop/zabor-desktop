import argparse
import hashlib
import json
import math
import subprocess
import zipfile
from pathlib import Path

import imageio_ffmpeg
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
SOURCE_DIR = ROOT / "scripts" / "ui-sound-sources" / "kenney"
DEFAULT_OUTPUT = ROOT / "src" / "renderer" / "src" / "assets" / "sounds" / "candidates"
RATE = 96000
OUTPUT_RATE = 48000
FFMPEG = imageio_ffmpeg.get_ffmpeg_exe()
RNG = np.random.default_rng(20261104)


def ffmpeg(arguments, audio=None):
    result = subprocess.run(
        [FFMPEG, "-hide_banner", "-loglevel", "error", *arguments],
        input=audio,
        capture_output=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))
    return result.stdout


def decode(path, rate=RATE, channels=1):
    data = ffmpeg(["-i", str(path), "-f", "f32le", "-ac", str(channels), "-ar", str(rate), "pipe:1"])
    result = np.frombuffer(data, dtype="<f4").astype(np.float64)
    return result if channels == 1 else result.reshape(-1, channels)


def unit(audio):
    peak = float(np.max(np.abs(audio)))
    return audio / peak if peak > 1e-9 else audio.copy()


def fade(audio, attack=0.002, release=0.015, rate=RATE):
    audio = audio.copy()
    start = min(len(audio), max(1, round(attack * rate)))
    end = min(len(audio), max(1, round(release * rate)))
    envelope = np.ones(len(audio))
    envelope[:start] *= np.sin(np.linspace(0, np.pi / 2, start)) ** 2
    envelope[-end:] *= np.cos(np.linspace(0, np.pi / 2, end)) ** 2
    return audio * (envelope[:, None] if audio.ndim == 2 else envelope)


def active_sample(audio, threshold_db=-46, tail=0.035):
    activity = np.flatnonzero(np.abs(audio) > np.max(np.abs(audio)) * 10 ** (threshold_db / 20))
    if not len(activity):
        raise ValueError("Source sample is silent")
    start = max(0, int(activity[0]) - round(0.002 * RATE))
    end = min(len(audio), int(activity[-1]) + round(tail * RATE))
    return fade(audio[start:end])


def process(audio, filters, channels=1, input_rate=RATE, output_rate=RATE):
    data = ffmpeg(
        ["-f", "f32le", "-ar", str(input_rate), "-ac", str(channels), "-i", "pipe:0", "-af", filters,
         "-f", "f32le", "-ar", str(output_rate), "-ac", str(channels), "pipe:1"],
        np.asarray(audio, dtype="<f4").tobytes(),
    )
    output = np.frombuffer(data, dtype="<f4").astype(np.float64)
    return output if channels == 1 else output.reshape(-1, channels)


def warp(audio, semitones=0, tempo=1, lowpass=7000):
    filters = (
        f"rubberband=pitch={2 ** (semitones / 12):.8f}:tempo={tempo}:transients=mixed:window=short,"
        f"highpass=f=90,lowpass=f={lowpass}"
    )
    return fade(unit(process(np.pad(audio, (0, round(0.08 * RATE))), filters)))


def fit(audio, duration):
    size = round(duration * RATE)
    return fade(np.pad(audio[:size], (0, max(0, size - len(audio)))))


def mix(layers, duration):
    output = np.zeros(round(duration * RATE))
    for offset, audio, gain in layers:
        start = round(offset * RATE)
        if start < 0 or start >= len(output):
            raise ValueError("Layer start is outside the sound")
        count = min(len(audio), len(output) - start)
        output[start:start + count] += audio[:count] * gain
    return output


def noise(duration, low, high, decay):
    size = round(duration * RATE)
    audio = RNG.normal(0, 1, size)
    frequencies = np.fft.rfftfreq(size, 1 / RATE)
    response = (1 - np.exp(-(frequencies / low) ** 4)) * np.exp(-(frequencies / high) ** 4)
    audio = np.fft.irfft(np.fft.rfft(audio) * response, n=size)
    return fade(unit(audio) * np.exp(-np.arange(size) / RATE / decay), attack=0.001, release=0.025)


def rubber(duration, start_hz, end_hz, bounce=0.13, formant=1100):
    time = np.arange(round(duration * RATE)) / RATE
    progress = time / duration
    pitch = end_hz + (start_hz - end_hz) * np.exp(-progress * 5.5)
    pitch *= 1 + bounce * np.sin(2 * np.pi * 10.5 * time) * np.exp(-time / 0.12)
    phase = np.cumsum(pitch) * 2 * np.pi / RATE
    modulation = 0.8 * np.sin(phase * 2.03) * np.exp(-time / 0.045)
    audio = np.zeros(len(time))
    for harmonic in range(1, 9):
        weight = np.exp(-0.5 * ((harmonic * pitch - formant) / 700) ** 2) / harmonic ** 0.85
        audio += weight * np.sin(harmonic * phase + modulation / harmonic)
    audio = np.tanh(audio * 1.8)
    audio *= (1 - np.exp(-time / 0.0017)) * np.exp(-time / (duration / 4.5))
    return fade(unit(audio), attack=0.0015, release=0.025)


def spring(duration, frequency, bend):
    time = np.arange(round(duration * RATE)) / RATE
    pitch = frequency * (1 + bend * np.exp(-time / 0.06))
    pitch *= 1 + 0.025 * np.sin(2 * np.pi * 23 * time) * np.exp(-time / 0.12)
    phase = np.cumsum(pitch) * 2 * np.pi / RATE
    audio = np.zeros(len(time))
    for ratio, amplitude, decay in [(1, 1, 0.13), (1.47, 0.32, 0.08), (2.09, 0.23, 0.055), (3.31, 0.11, 0.04)]:
        audio += amplitude * np.sin(phase * ratio) * np.exp(-time / decay)
    audio *= 1 - np.exp(-time / 0.002)
    return fade(unit(audio), release=0.03)


def string_pluck(duration, frequency):
    size = round(duration * RATE)
    period = round(RATE / frequency)
    buffer = RNG.uniform(-1, 1, period)
    buffer -= buffer.mean()
    audio = np.empty(size)
    for index in range(size):
        slot = index % period
        value = buffer[slot]
        buffer[slot] = 0.996 * (value + buffer[(slot + 1) % period]) * 0.5
        audio[index] = value
    audio = process(audio, "highpass=f=150,lowpass=f=4600")
    return fade(unit(audio), attack=0.001, release=0.035)


def spatialize(audio):
    stereo = np.repeat(audio[:, None], 2, axis=1)
    for channel, delay, gain in [(0, 0.013, 0.065), (1, 0.019, 0.065), (0, 0.041, 0.025), (1, 0.047, 0.025)]:
        shift = round(delay * RATE)
        stereo[shift:, channel] += audio[:-shift] * gain
    return stereo


def master(audio, target_peak):
    audio = np.tanh(audio * 1.12) / 1.12
    stereo = spatialize(audio)
    stereo = process(stereo, "highpass=f=95,lowpass=f=7600,equalizer=f=2800:t=q:w=0.8:g=-1.5", channels=2, output_rate=OUTPUT_RATE)
    stereo -= stereo.mean(axis=0)
    stereo = fade(stereo, attack=0.0015, release=0.025, rate=OUTPUT_RATE)
    return unit(stereo) * 10 ** (target_peak / 20)


def encode(path, audio, codec):
    ffmpeg(
        ["-y", "-f", "f32le", "-ar", str(OUTPUT_RATE), "-ac", "2", "-i", "pipe:0", *codec, str(path)],
        np.asarray(audio, dtype="<f4").tobytes(),
    )


def measure(path):
    audio = decode(path, rate=OUTPUT_RATE, channels=2)
    if not np.isfinite(audio).all():
        raise ValueError(f"Non-finite samples in {path.name}")
    peak = float(np.max(np.abs(audio)))
    if peak >= 0.8:
        raise ValueError(f"Insufficient headroom in {path.name}")
    mono = audio.mean(axis=1)
    activity = np.flatnonzero(np.abs(mono) > 10 ** (-55 / 20))
    if not len(activity):
        raise ValueError(f"Silent output: {path.name}")
    return {
        "duration_s": round(len(audio) / OUTPUT_RATE, 4),
        "peak_dbfs": round(20 * math.log10(max(peak, 1e-12)), 2),
        "rms_dbfs": round(20 * math.log10(max(float(np.sqrt(np.mean(audio ** 2))), 1e-12)), 2),
        "onset_ms": round(float(activity[0] / OUTPUT_RATE * 1000), 2),
        "dc_offset": round(float(np.max(np.abs(audio.mean(axis=0)))), 8),
        "stereo_correlation": round(float(np.corrcoef(audio.T)[0, 1]), 4),
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }


def build_sounds():
    sources = {name: active_sample(decode(SOURCE_DIR / f"{name}.ogg")) for name in ["click_003", "switch_002", "pluck_001", "bong_001", "scratch_002"]}
    join = decode(ROOT / "src" / "renderer" / "src" / "assets" / "sounds" / "join.mp3")
    join_low = fade(unit(join[round(0.302 * RATE):round(0.442 * RATE)]))
    join_high = fade(unit(join[round(0.575 * RATE):round(0.815 * RATE)]))
    click = fit(warp(sources["click_003"], -4, 1.5), 0.045)
    toggle_low = fit(warp(sources["switch_002"], -7, 1.5), 0.09)
    toggle_high = fit(warp(sources["switch_002"], 3, 1.6), 0.075)
    pluck = fit(warp(sources["pluck_001"], -3, 1.65), 0.19)
    bonk = fit(warp(sources["bong_001"], -9, 1.7, 3600), 0.23)
    scratch = fit(warp(sources["scratch_002"], -4, 2.1, 4200), 0.12)
    mute = mix([
        (0, toggle_low, 0.24),
        (0.008, rubber(0.19, 430, 180, 0.09, 700), 0.48),
        (0.025, fit(warp(join_low, -9, 1.5, 3500), 0.13), 0.13),
        (0.013, noise(0.08, 550, 2800, 0.014), 0.085),
        (0.10, spring(0.20, 265, -0.28), 0.10),
    ], 0.34)
    unmute = mix([
        (0, toggle_high, 0.20),
        (0.006, rubber(0.16, 210, 590, 0.11, 1250), 0.38),
        (0.064, pluck, 0.25),
        (0.08, fit(warp(join_low, 2, 1.4), 0.14), 0.15),
        (0.075, spring(0.23, 595, 0.30), 0.075),
        (0.008, noise(0.075, 900, 3700, 0.015), 0.045),
    ], 0.37)
    stream_start = mix([
        (0, click, 0.15),
        (0.016, fade(scratch[::-1], attack=0.045, release=0.012), 0.07),
        (0.025, rubber(0.24, 180, 565, 0.22, 1000), 0.30),
        (0.05, noise(0.16, 650, 4500, 0.11), 0.055),
        (0.155, fit(warp(join_low, -5, 1.3), 0.16), 0.25),
        (0.275, string_pluck(0.23, 595), 0.18),
        (0.385, fit(warp(join_high, -3, 1.05), 0.26), 0.35),
        (0.395, spring(0.25, 895, 0.25), 0.06),
    ], 0.74)
    stream_stop = mix([
        (0, click, 0.16),
        (0.012, fit(warp(join_high, -3, 1.4), 0.15), 0.27),
        (0.093, rubber(0.25, 730, 205, 0.14, 850), 0.33),
        (0.21, bonk, 0.30),
        (0.225, string_pluck(0.24, 298), 0.12),
        (0.23, spring(0.25, 230, -0.34), 0.09),
        (0.225, noise(0.075, 450, 2000, 0.017), 0.05),
    ], 0.63)
    message = mix([
        (0, fit(click, 0.022), 0.045),
        (0.003, rubber(0.105, 460, 790, 0.08, 1450), 0.19),
        (0.019, fit(warp(join_high, -3, 1.85, 5800), 0.15), 0.28),
        (0.064, fit(pluck, 0.11), 0.12),
        (0.056, spring(0.13, 610, 0.15), 0.045),
    ], 0.26)
    return [
        ("mute", mute, -11.5, "shared microphone/headphone off; sampled toggle, rubber formants, pitched join fragment, spring, air"),
        ("unmute", unmute, -11.0, "shared microphone/headphone on; sampled toggle/pluck, upward rubber formants, pitched join fragment, spring"),
        ("stream-start", stream_start, -9.5, "reverse sampled scratch, elastic rise, resampled join instrument, Karplus-Strong string, spring"),
        ("stream-stop", stream_stop, -10.5, "descending join instrument, rubber drop, sampled bonk, Karplus-Strong string, spring"),
        ("message", message, -17.5, "quiet rubber pop, short resampled join instrument, sampled pluck, spring"),
    ]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--overwrite", action="store_true")
    arguments = parser.parse_args()
    names = ["mute", "unmute", "stream-start", "stream-stop", "message", "preview"]
    outputs = [arguments.output / f"{name}.{extension}" for name in names for extension in ["wav", "mp3"]]
    outputs.append(arguments.output / "manifest.json")
    outputs.append(arguments.output / "zabor-ui-sounds.zip")
    if not arguments.overwrite and any(path.exists() for path in outputs):
        raise FileExistsError("Output already exists; use --overwrite only for previously generated files")
    arguments.output.mkdir(parents=True, exist_ok=True)
    manifest = {
        "style": "restrained cartoon/meme; original Zabor join instrument plus CC0 foley and physical-model layers",
        "render_rate_hz": RATE,
        "wav_rate_hz": OUTPUT_RATE,
        "mp3_rate_hz": 44100,
        "mp3_bitrate_kbps": 192,
        "seed": 20261104,
        "runtime_integration": False,
        "source_page": "https://kenney.nl/assets/interface-sounds",
        "source_license": "CC0-1.0",
        "sounds": {},
        "preview_sections": [],
    }
    preview_parts = [np.zeros((round(0.3 * OUTPUT_RATE), 2))]
    cursor = 0.3
    for name, dry, peak, recipe in build_sounds():
        audio = master(dry, peak)
        wav = arguments.output / f"{name}.wav"
        mp3 = arguments.output / f"{name}.mp3"
        encode(wav, audio, ["-ar", str(OUTPUT_RATE), "-c:a", "pcm_s24le"])
        encode(mp3, audio, ["-ar", "44100", "-c:a", "libmp3lame", "-b:a", "192k", "-id3v2_version", "3"])
        manifest["sounds"][name] = {"recipe": recipe, "wav": measure(wav), "mp3": measure(mp3)}
        manifest["preview_sections"].append({"name": name, "start_s": round(cursor, 3), "repetitions": 2})
        for _ in range(2):
            preview_parts.extend([audio, np.zeros((round(0.55 * OUTPUT_RATE), 2))])
            cursor += len(audio) / OUTPUT_RATE + 0.55
        preview_parts.append(np.zeros((round(0.55 * OUTPUT_RATE), 2)))
        cursor += 0.55
    preview = np.concatenate(preview_parts)
    encode(arguments.output / "preview.wav", preview, ["-c:a", "pcm_s24le"])
    encode(arguments.output / "preview.mp3", preview, ["-ar", "44100", "-c:a", "libmp3lame", "-b:a", "192k"])
    manifest["preview"] = measure(arguments.output / "preview.mp3")
    (arguments.output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    archive_files = [path for path in outputs if path.suffix != ".zip" and path.name != "preview.wav"]
    readme = arguments.output / "README.md"
    if readme.exists():
        archive_files.append(readme)
    with zipfile.ZipFile(arguments.output / "zabor-ui-sounds.zip", "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in archive_files:
            archive.write(path, arcname=path.name)
    print(json.dumps({"output": str(arguments.output), "sounds": manifest["sounds"], "preview_sections": manifest["preview_sections"]}, indent=2))


if __name__ == "__main__":
    main()
