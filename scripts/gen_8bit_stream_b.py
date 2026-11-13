"""Second 8-bit stream for the merge benchmark.

Deliberately UNCORRELATED with sweep_31s_2c_44sr_8bd.wav: a different
fundamental, a different sweep direction and a different seed, so the
mix exercises clamping realistically rather than summing a signal with
a near-copy of itself.

Driven near full scale on purpose: merge clamps, and two loud streams
are exactly the case where saturation is the common path rather than an
edge case.
"""
import wave, struct, math, random

SR, CH, DUR = 44100, 2, 31.0
PEAK = 115
frames = int(SR * DUR)
rnd = random.Random(99991)

buf = bytearray()
phase = 0.0
for i in range(frames):
    t = i / SR
    # Descending sweep, versus stream A's ascending one.
    f = 2600.0 * (90.0 / 2600.0) ** ((t / DUR) % 1.0)
    phase += 2 * math.pi * f / SR
    env = 0.55 + 0.45 * math.sin(2 * math.pi * 0.07 * t + 1.1)
    base = math.sin(phase) * env
    l = int(PEAK * base) + rnd.randint(-4, 4)
    r = int(PEAK * base * 0.9) + rnd.randint(-4, 4)
    buf += struct.pack('<bb', max(-128, min(127, l)), max(-128, min(127, r)))

out = 'raw_audios/sweep_31s_2c_44sr_8bd_streamB.wav'
w = wave.open(out, 'wb')
w.setnchannels(CH); w.setsampwidth(1); w.setframerate(SR)
w.writeframes(bytes((b + 128) & 0xFF for b in buf))
w.close()
print(f"wrote {out}: {frames*CH} samples, {frames/SR:.2f}s")
