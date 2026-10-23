#!/usr/bin/env python3
"""Synthesize Coo's sound themes (new mail + meeting reminder per theme) into crates/pigeon-bin/assets/sounds/.

Short, soft and low-level on purpose (peak about -14 dBFS, under a second):
a sound that says "mail" without pulling you out of what you're doing.
Pure stdlib; rerun after tweaking a recipe.
"""
import math, os, random, struct, wave

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


def hoo(u, dur, f, rise=1.07, fall=0.9):
    """One dove 'hoo': hollow, nearly pure tone that swoops up then sinks,
    soft attack, rounded end, a little breath. No harmonics swell, so it
    doesn't turn into a 'wah'."""
    if u < 0 or u > dur:
        return 0.0
    x = u / dur
    # pitch: up quickly to `rise`, then drift down to `fall`
    curve = rise - (rise - fall) * x ** 1.5 if x > 0.18 else 1 + (rise - 1) * (x / 0.18)
    phase = 2 * math.pi * f * u * curve
    a = min(1.0, u / 0.035) * (1 - x) ** 0.8
    tone = math.sin(phase) + 0.05 * math.sin(2 * phase)
    return a * (tone + 0.07 * BREATH[int(u * RATE) % len(BREATH)])


def coo(t):
    # Mourning-dove style "hoo-HOOOO": a short low note, then a longer swoop.
    return 0.6 * hoo(t, 0.16, 330.0) + hoo(t - 0.22, 0.46, 380.0, rise=1.1, fall=0.86)


def chime(t):
    return bell(1318.5, 0.9, 0.22)(t) * 0.8 + bell(1975.5, 0.9, 0.28, start=0.11)(t) * 0.6


def bowl(t):
    return bell(784.0, 1.2, 0.42, partials=((1, 1.0), (2.0, 0.08), (2.98, 0.05)), attack=0.012)(t)


def wood(t):
    # Soft marimba-ish knock, two quick notes.
    p = ((1, 1.0), (3.9, 0.12))
    return bell(988.0, 0.35, 0.06, partials=p)(t) + bell(1318.5, 0.35, 0.07, partials=p, start=0.09)(t) * 0.8


def droplet_meeting(t):
    # Three drips, each a little higher: same sound, reads as "time to go".
    return sum(drip(t - st, 700.0 * r, 1700.0 * r) for st, r in ((0.0, 1.0), (0.17, 1.19), (0.34, 1.41)))


def coo_meeting(t):
    # The full dove phrase "hoo-HOOOO hoo hoo", the long note a touch higher.
    return (0.6 * hoo(t, 0.16, 330.0) + hoo(t - 0.22, 0.5, 400.0, rise=1.1, fall=0.86)
            + 0.7 * hoo(t - 0.86, 0.2, 340.0) + 0.6 * hoo(t - 1.14, 0.2, 330.0))


def chime_meeting(t):
    # Rising three-note arpeggio on the same bell.
    return sum(bell(f, 1.1, 0.24, start=st)(t) * a for f, st, a in ((1318.5, 0.0, 0.8), (1661.2, 0.13, 0.7), (1975.5, 0.26, 0.65)))


def bowl_meeting(t):
    # The bowl struck twice, second a fifth higher.
    p = ((1, 1.0), (2.0, 0.08), (2.98, 0.05))
    return bell(784.0, 1.6, 0.42, partials=p, attack=0.012)(t) + bell(1174.7, 1.6, 0.42, partials=p, attack=0.012, start=0.35)(t) * 0.8


def wood_meeting(t):
    p = ((1, 1.0), (3.9, 0.12))
    return sum(bell(f, 0.4, 0.06, partials=p, start=st)(t) for f, st in ((988.0, 0.0), (1174.7, 0.1), (1318.5, 0.2), (1568.0, 0.3)))


# theme -> (mail recipe, length), (meeting recipe, length)
SOUNDS = {
    "droplet": ((droplet, 0.3), (droplet_meeting, 0.6)),
    "coo": ((coo, 0.72), (coo_meeting, 1.4)),
    "chime": ((chime, 0.95), (chime_meeting, 1.3)),
    "knock": ((wood, 0.5), (wood_meeting, 0.75)),
    "bowl": ((bowl, 1.25), (bowl_meeting, 1.9)),
}


def render(fn, dur):
    n = int(RATE * dur)
    xs = [fn(i / RATE) for i in range(n)]
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
    for name, pair in SOUNDS.items():
        for kind, (fn, dur) in zip(("mail", "meeting"), pair):
            xs = render(fn, dur)
            path = os.path.join(OUT, f"{name}-{kind}.wav")
            with wave.open(path, "wb") as w:
                w.setnchannels(1)
                w.setsampwidth(2)
                w.setframerate(RATE)
                w.writeframes(b"".join(struct.pack("<h", int(x * 32767)) for x in xs))
            print(path, os.path.getsize(path))


if __name__ == "__main__":
    main()
