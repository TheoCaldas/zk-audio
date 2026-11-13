"""Four 8-bit fixtures spanning {1,2} channels x {22050,44100} Hz.

For the rate/channel decomposition experiment: N = channels * sample_rate * T,
so the same N can be reached by four different (ch, f_s, T) routes. These
fixtures let the suite prove each route and confirm the prover sees only N.

Each file is long enough for the largest case it is used in (~5s), and uses
the same near-full-scale content as the other 8-bit fixtures so gain_k2's
clamp is exercised identically across all four.

The per-sample content is generated from absolute time t, NOT from sample
index, so the 22.05kHz files are a genuine resampling of the same signal
rather than a different waveform. That keeps the comparison about rate and
channel count, not about what the audio happens to contain.
"""
import wave, struct, math, random

DUR = 5.0
PEAK = 120                       # of 127 -> k=2 saturates often

for ch in (1, 2):
    for sr in (22050, 44100):
        frames = int(sr * DUR)
        rnd = random.Random(20261110)   # same seed everywhere
        buf = bytearray()
        for i in range(frames):
            t = i / sr
            # Phase integrated in closed form from t, so the waveform is the
            # same function of time at either rate.
            f0, f1 = 110.0, 3000.0
            k = (f1 / f0) ** (t / DUR)
            phase = 2 * math.pi * f0 * DUR * (k - 1) / math.log(f1 / f0)
            env = 0.6 + 0.4 * math.sin(2 * math.pi * 0.11 * t)
            base = math.sin(phase) * env
            l = max(-128, min(127, int(PEAK * base) + rnd.randint(-3, 3)))
            if ch == 1:
                buf += struct.pack('<b', l)
            else:
                r = max(-128, min(127, int(PEAK * base * 0.85) + rnd.randint(-3, 3)))
                buf += struct.pack('<bb', l, r)

        out = f'raw_audios/rc_{ch}c_{sr}sr_8bd.wav'
        w = wave.open(out, 'wb')
        w.setnchannels(ch); w.setsampwidth(1); w.setframerate(sr)
        # wave writes 8-bit as UNSIGNED; convert signed -> unsigned storage.
        w.writeframes(bytes((b + 128) & 0xFF for b in buf))
        w.close()
        print(f"wrote {out}: {frames} frames x {ch}ch = {frames*ch} samples, {DUR}s @ {sr}Hz")
