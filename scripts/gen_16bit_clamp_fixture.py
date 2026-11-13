"""Deterministic 31-second 16-bit/44.1kHz stereo WAV fixture.

The bd=16 counterpart of gen_8bit_bench_fixture.py, built to the same recipe
so the two depths are compared on like material: a log sweep plus dither,
driven NEAR FULL SCALE.

Near-full-scale is deliberate. The gain_* circuits clamp, so saturation is
part of the relation under test and the benchmark should exercise it. This
is the opposite of gen_bench_fixture.py, which caps at 1/4 scale because the
older gain2 relation has NO clamping and would overflow i16.

Length covers N=2,646,000 (1,764 folds at B=100, bd=16) = exactly 30s.
"""
import wave, struct, math, random

SR, CH, DUR = 44100, 2, 31.0
PEAK = 30720                    # of 32767 -> k=2 saturates often
frames = int(SR * DUR)
rnd = random.Random(20261024)   # same seed as the 8-bit fixture

buf = bytearray()
phase = 0.0
for i in range(frames):
    t = i / SR
    f = 110.0 * (3000.0 / 110.0) ** ((t / DUR) % 1.0)   # log sweep
    phase += 2 * math.pi * f / SR
    env = 0.6 + 0.4 * math.sin(2 * math.pi * 0.11 * t)
    base = math.sin(phase) * env
    l = int(PEAK * base) + rnd.randint(-768, 768)
    r = int(PEAK * base * 0.85) + rnd.randint(-768, 768)
    l = max(-32768, min(32767, l))
    r = max(-32768, min(32767, r))
    buf += struct.pack('<hh', l, r)

out = 'raw_audios/sweep_31s_2c_44sr_16bd_fullscale.wav'
w = wave.open(out, 'wb')
w.setnchannels(CH); w.setsampwidth(2); w.setframerate(SR)
w.writeframes(bytes(buf))
w.close()
print(f"wrote {out}: {frames} frames, {frames*CH} samples, {frames/SR:.2f}s")
