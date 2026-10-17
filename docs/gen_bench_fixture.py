"""Deterministic 2-minute 16-bit/44.1kHz stereo WAV fixture for the bench suite.

Content is a log sweep plus low-level dither noise, seeded, so the file can be
regenerated bit-identically. Amplitude is capped at 1/4 full scale because the
gain2 relation the circuit proves is `x*2` with NO clamping -- samples near
full scale would overflow i16 on the Rust side and stop matching the circuit.
"""
import wave, struct, math, random

SR, CH, DUR = 44100, 2, 125.0   # 125s > 120s target, small headroom
PEAK = 8000                      # *2 = 16000, safely inside i16
frames = int(SR * DUR)
rnd = random.Random(20261016)

buf = bytearray()
phase = 0.0
for i in range(frames):
    t = i / SR
    f = 80.0 * (4000.0 / 80.0) ** (t / DUR)      # log sweep 80Hz -> 4kHz
    phase += 2 * math.pi * f / SR
    env = 0.55 + 0.45 * math.sin(2 * math.pi * 0.08 * t)
    base = math.sin(phase) * env
    l = int(PEAK * base + rnd.randint(-120, 120))
    r = int(PEAK * base * 0.82 + rnd.randint(-120, 120))
    l = max(-PEAK, min(PEAK, l))
    r = max(-PEAK, min(PEAK, r))
    buf += struct.pack('<hh', l, r)

out = 'raw_audios/sweep_120s_2c_44sr_16bd.wav'
w = wave.open(out, 'wb')
w.setnchannels(CH); w.setsampwidth(2); w.setframerate(SR)
w.writeframes(bytes(buf))
w.close()
print(f"wrote {out}: {frames} frames, {frames*CH} samples, {frames/SR:.2f}s")
