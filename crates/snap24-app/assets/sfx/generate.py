#!/usr/bin/env python3
"""Generate Snap 24's UI sounds. Run from this directory:

    python3 generate.py

Writes deal.wav, merge.wav, win.wav, lose.wav (mono 44.1kHz 16-bit). Kept in the
repo so the sounds are reproducible; they're tiny and deliberately quiet.
"""

import wave
from pathlib import Path

import numpy as np

RATE = 44_100


def envelope(n, attack=0.005, decay=8.0):
    """Fast attack, exponential decay."""
    t = np.linspace(0.0, n / RATE, n, endpoint=False)
    a = np.clip(t / max(attack, 1e-6), 0.0, 1.0)
    return a * np.exp(-decay * t)


def tone(freq, n, detune=0.0):
    t = np.linspace(0.0, n / RATE, n, endpoint=False)
    x = np.sin(2 * np.pi * freq * t)
    if detune:
        x += detune * np.sin(2 * np.pi * freq * 2 * t)
    return x


def noise(n, smoothing=8):
    x = np.random.default_rng(7).normal(0.0, 1.0, n)
    kernel = np.ones(smoothing) / smoothing
    return np.convolve(x, kernel, mode="same")


def save(name, samples):
    samples = np.clip(samples, -1.0, 1.0)
    pcm = (samples * 0.9 * 32767).astype("<i2")
    with wave.open(str(Path(__file__).parent / name), "wb") as fh:
        fh.setnchannels(1)
        fh.setsampwidth(2)
        fh.setframerate(RATE)
        fh.writeframes(pcm.tobytes())


def deal():
    """A soft card slide: brief filtered noise with a downward tilt."""
    n = int(0.16 * RATE)
    body = noise(n, smoothing=24) * envelope(n, attack=0.008, decay=18.0)
    sweep = tone(520, n) * np.linspace(1.0, 0.35, n) * envelope(n, 0.006, 22.0)
    save("deal.wav", 0.6 * body + 0.35 * sweep)


def merge():
    """A chip clack: two very short clicks."""
    gap = int(0.05 * RATE)
    click_n = int(0.03 * RATE)
    click = (tone(1500, click_n) + 0.8 * noise(click_n, smoothing=3)) * envelope(
        click_n, attack=0.001, decay=70.0
    )
    out = np.zeros(gap * 2 + click_n)
    out[:click_n] += click
    out[gap : gap + click_n] += 0.7 * click
    save("merge.wav", 0.5 * out)


def win():
    """A short rising arpeggio."""
    notes = [523.25, 659.25, 783.99, 1046.5]
    step = int(0.10 * RATE)
    out = np.zeros(step * len(notes) + int(0.25 * RATE))
    for i, freq in enumerate(notes):
        seg = int(0.28 * RATE)
        x = tone(freq, seg, detune=0.15) * envelope(seg, attack=0.004, decay=9.0)
        out[i * step : i * step + seg] += 0.35 * x
    save("win.wav", out)


def lose():
    """Two descending notes, soft."""
    out = np.zeros(int(0.5 * RATE))
    for i, freq in enumerate([392.0, 311.13]):
        start = i * int(0.18 * RATE)
        seg = int(0.32 * RATE)
        x = tone(freq, seg, detune=0.1) * envelope(seg, attack=0.01, decay=7.0)
        out[start : start + seg] += 0.4 * x
    save("lose.wav", out)


if __name__ == "__main__":
    np.random.seed(24)
    deal()
    merge()
    win()
    lose()
    print("wrote deal/merge/win/lose.wav")
