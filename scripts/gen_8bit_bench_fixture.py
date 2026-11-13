"""Deterministic 31-second 8-bit/44.1kHz stereo WAV fixture.

Length covers N=2,644,300 (853 folds at B=100, bd=8), the largest whole
number of steps inside 30s of stereo audio.

Unlike the 16-bit gain2 fixture, this one is deliberately driven NEAR FULL
SCALE: the gain_* circuits clamp, so saturation is part of the relation
under test and the benchmark should exercise it rather than avoid it.
"""
import wave, struct, math, random

SR, CH, DUR = 44100, 2, 31.0
PEAK = 120                      # of 127 -> k=2 saturates often
frames = int(SR * DUR)
rnd = random.Random(20261024)

buf = bytearray()
phase = 0.0
for i in range(frames):
    t = i / SR
    f = 110.0 * (3000.0 / 110.0) ** ((t / DUR) % 1.0)   # log sweep
    phase += 2 * math.pi * f / SR
    env = 0.6 + 0.4 * math.sin(2 * math.pi * 0.11 * t)
    base = math.sin(phase) * env
    l = int(PEAK * base) + rnd.randint(-3, 3)
    r = int(PEAK * base * 0.85) + rnd.randint(-3, 3)
    l = max(-128, min(127, l))
    r = max(-128, min(127, r))
    buf += struct.pack('<bb', l, r)

out = 'raw_audios/sweep_31s_2c_44sr_8bd.wav'
w = wave.open(out, 'wb')
w.setnchannels(CH); w.setsampwidth(1); w.setframerate(SR)
# wave writes 8-bit as UNSIGNED; convert signed -> unsigned storage.
w.writeframes(bytes((b + 128) & 0xFF for b in buf))
w.close()
print(f"wrote {out}: {frames} frames, {frames*CH} samples, {frames/SR:.2f}s")
