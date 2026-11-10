"""Deterministic 35-second 24-bit/44.1kHz stereo WAV fixture.

Length covers N=2,650,500 (2,945 folds at B=100, bd=24), the largest case
in the bit-depth comparison sweep, with headroom.

Like the 8-bit fixture this is driven NEAR FULL SCALE: the gain_* circuits
clamp, so saturation is part of the relation under test and the benchmark
should exercise it rather than avoid it.

24-bit WAV has no unsigned-storage quirk (that is 8-bit only), so samples
are written signed, three bytes little-endian per sample.
"""
import wave, struct, math, random

SR, CH, DUR = 44100, 2, 35.0
PEAK = 7_900_000                 # of 8,388,607 -> k=2 saturates often
frames = int(SR * DUR)
rnd = random.Random(20261110)

buf = bytearray()
phase = 0.0
for i in range(frames):
    t = i / SR
    f = 110.0 * (3000.0 / 110.0) ** ((t / DUR) % 1.0)   # log sweep
    phase += 2 * math.pi * f / SR
    env = 0.6 + 0.4 * math.sin(2 * math.pi * 0.11 * t)
    base = math.sin(phase) * env
    l = int(PEAK * base) + rnd.randint(-2000, 2000)
    r = int(PEAK * base * 0.85) + rnd.randint(-2000, 2000)
    l = max(-8388608, min(8388607, l))
    r = max(-8388608, min(8388607, r))
    # 24-bit little-endian: low three bytes of the signed value.
    buf += struct.pack('<i', l)[:3]
    buf += struct.pack('<i', r)[:3]

out = 'raw_audios/sweep_35s_2c_44sr_24bd.wav'
w = wave.open(out, 'wb')
w.setnchannels(CH); w.setsampwidth(3); w.setframerate(SR)
w.writeframes(bytes(buf))
w.close()
print(f"wrote {out}: {frames} frames, {frames*CH} samples, {frames/SR:.2f}s")
