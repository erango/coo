#!/usr/bin/env python3
"""Synthesize Coo's sound themes (new mail + meeting reminder per theme) into crates/pigeon-bin/assets/sounds/.

Short, soft and low-level on purpose (peak about -14 dBFS, a second or two;
the bells ring out and fade rather than stop): a sound that says "mail"
without pulling you out of what you're doing.
Pure stdlib; rerun after tweaking a recipe (`gen-sounds.py chime bowl` redoes
only those themes).
"""
import math, os, random, struct, sys, wave

RATE = 44100
OUT = os.path.join(os.path.dirname(__file__), "..", "crates", "pigeon-bin", "assets", "sounds")
PEAK = 10 ** (-14 / 20)


def env(t, attack, decay):
    """Fast fade-in, exponential tail."""
    a = min(1.0, t / attack) if attack > 0 else 1.0
    return a * math.exp(-t / decay)


def bell(freq, dur, decay, partials=((1, 1.0), (2.76, 0.18), (5.4, 0.06)), attack=0.004, start=0.0):
    def f(t):
        t -= start
        if t < 0 or t > dur:
            return 0.0
        return env(t, attack, decay) * sum(a * math.sin(2 * math.pi * freq * m * t) for m, a in partials)
    return f


def drip(u, f0=700.0, f1=1700.0, decay=0.045):
    """One water drip: a bubble resonance whose pitch rises fast (the "plink"
    of a drop hitting water), with a very short tail."""
    if u < 0 or u > 0.2:
        return 0.0
    k = 55.0  # how fast the pitch sweeps up
    phase = 2 * math.pi * (f1 * u - (f1 - f0) * (1 - math.exp(-k * u)) / k)
    return env(u, 0.0015, decay) * math.sin(phase)


def droplet(t):
    # A drip, then a fainter, higher one: a single drop and its ripple.
    return drip(t) + 0.28 * drip(t - 0.11, 900.0, 2100.0, 0.03)


def _breath(n=RATE * 3):
    """Soft low-passed noise for the airy part of a dove's coo."""
    rnd = random.Random(7)
    out, y = [], 0.0
    for _ in range(n):
        y += 0.06 * (rnd.uniform(-1, 1) - y)
        out.append(y)
    m = max(abs(v) for v in out) or 1.0
    return [v / m for v in out]


BREATH = _breath()


def hoo_samples(dur, f, rise=1.07, fall=0.9):
    """One dove 'hoo' as samples: hollow, nearly pure tone that glides up then
    sinks. Phase is integrated sample by sample and the pitch curve is smooth,
    so there are no jumps (clicks); attack and release are raised-cosine."""
    n = int(RATE * dur)
    out, phase = [], 0.0
    for i in range(n):
        x = i / n
        # smooth glide: up to `rise` by 25%, then ease down to `fall`
        if x < 0.25:
            k = 0.5 - 0.5 * math.cos(math.pi * x / 0.25)
            ratio = 1 + (rise - 1) * k
        else:
            k = 0.5 - 0.5 * math.cos(math.pi * (x - 0.25) / 0.75)
            ratio = rise + (fall - rise) * k
        phase += 2 * math.pi * f * ratio / RATE
        att = 0.5 - 0.5 * math.cos(math.pi * min(1.0, x / 0.12))
        rel = 0.5 + 0.5 * math.cos(math.pi * max(0.0, (x - 0.55) / 0.45))
        a = att * rel
        tone = math.sin(phase) + 0.05 * math.sin(2 * phase)
        out.append(a * (tone + 0.07 * BREATH[i % len(BREATH)]))
    return out


def phrase(notes, total):
    """Mix (start s, gain, samples) notes into one buffer of `total` seconds."""
    buf = [0.0] * int(RATE * total)
    for start, gain, smp in notes:
        o = int(RATE * start)
        for k, v in enumerate(smp):
            if o + k < len(buf):
                buf[o + k] += gain * v
    return buf


_COO = phrase([(0.0, 0.6, hoo_samples(0.2, 330.0)), (0.2, 1.0, hoo_samples(0.48, 380.0, rise=1.1, fall=0.86))], 0.72)
_COO_MEETING = phrase(
    [
        (0.0, 0.6, hoo_samples(0.2, 330.0)),
        (0.2, 1.0, hoo_samples(0.52, 400.0, rise=1.1, fall=0.86)),
        (0.8, 0.7, hoo_samples(0.24, 340.0)),
        (1.08, 0.6, hoo_samples(0.24, 330.0)),
    ],
    1.4,
)


def coo(t):
    # Mourning-dove style "hoo-HOOOO": a short low note flowing into a swoop.
    i = int(t * RATE)
    return _COO[i] if i < len(_COO) else 0.0


def chime(t):
    return bell(1318.5, 1.7, 0.32)(t) * 0.8 + bell(1975.5, 1.7, 0.38, start=0.11)(t) * 0.6


def bowl(t):
    return bell(784.0, 2.4, 0.6, partials=((1, 1.0), (2.0, 0.08), (2.98, 0.05)), attack=0.012)(t)


def wood(t):
    # Soft marimba-ish knock, two quick notes.
    p = ((1, 1.0), (3.9, 0.12))
    return bell(988.0, 0.35, 0.06, partials=p)(t) + bell(1318.5, 0.35, 0.07, partials=p, start=0.09)(t) * 0.8


def droplet_meeting(t):
    # Three drips, each a little higher: same sound, reads as "time to go".
    return sum(drip(t - st, 700.0 * r, 1700.0 * r) for st, r in ((0.0, 1.0), (0.17, 1.19), (0.34, 1.41)))


def coo_meeting(t):
    # The full dove phrase "hoo-HOOOO hoo hoo", the long note a touch higher.
    i = int(t * RATE)
    return _COO_MEETING[i] if i < len(_COO_MEETING) else 0.0


def chime_meeting(t):
    # Rising three-note arpeggio on the same bell.
    return sum(bell(f, 2.0, 0.34, start=st)(t) * a for f, st, a in ((1318.5, 0.0, 0.8), (1661.2, 0.13, 0.7), (1975.5, 0.26, 0.65)))


def bowl_meeting(t):
    # The bowl struck twice, second a fifth higher.
    p = ((1, 1.0), (2.0, 0.08), (2.98, 0.05))
    return bell(784.0, 3.0, 0.6, partials=p, attack=0.012)(t) + bell(1174.7, 3.0, 0.6, partials=p, attack=0.012, start=0.35)(t) * 0.8


def wood_meeting(t):
    p = ((1, 1.0), (3.9, 0.12))
    return sum(bell(f, 0.4, 0.06, partials=p, start=st)(t) for f, st in ((988.0, 0.0), (1174.7, 0.1), (1318.5, 0.2), (1568.0, 0.3)))


# theme -> (mail recipe, length), (meeting recipe, length)
# Bells that ring out: the last part of the file fades smoothly to silence.
RING_OUT = {"chime", "bowl"}
SOUNDS = {
    "droplet": ((droplet, 0.3), (droplet_meeting, 0.6)),
    "coo": ((coo, 0.72), (coo_meeting, 1.4)),
    "chime": ((chime, 1.6), (chime_meeting, 2.0)),
    "knock": ((wood, 0.5), (wood_meeting, 0.75)),
    "bowl": ((bowl, 2.3), (bowl_meeting, 3.0)),
}


def render(fn, dur, ring_out=False):
    n = int(RATE * dur)
    xs = [fn(i / RATE) for i in range(n)]
    if ring_out:
        # A cosine fade over the last 45%, so the tail dies away instead of
        # being cut off at the end of the file.
        f = int(n * 0.45)
        for i in range(f):
            xs[n - f + i] *= 0.5 * (1 + math.cos(math.pi * i / f))
    # 3 ms fade at both ends so nothing clicks.
    f = int(RATE * 0.003)
    for i in range(f):
        xs[i] *= i / f
        xs[-1 - i] *= i / f
    m = max(abs(x) for x in xs) or 1.0
    return [x / m * PEAK for x in xs]


def main():
    random.seed(1)
    os.makedirs(OUT, exist_ok=True)
    only = set(sys.argv[1:])
    for name, pair in SOUNDS.items():
        if only and name not in only:
            continue
        for kind, (fn, dur) in zip(("mail", "meeting"), pair):
            xs = render(fn, dur, name in RING_OUT)
            path = os.path.join(OUT, f"{name}-{kind}.wav")
            with wave.open(path, "wb") as w:
                w.setnchannels(1)
                w.setsampwidth(2)
                w.setframerate(RATE)
                w.writeframes(b"".join(struct.pack("<h", int(x * 32767)) for x in xs))
            print(path, os.path.getsize(path))


if __name__ == "__main__":
    main()
