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


def droplet(t):
    # Sine whose pitch falls quickly, like a drop into water.
    if t > 0.32:
        return 0.0
    f0, f1 = 1250.0, 620.0
    k = 18.0
    phase = 2 * math.pi * (f1 * t + (f0 - f1) * (1 - math.exp(-k * t)) / k)
    return env(t, 0.003, 0.07) * math.sin(phase)


def coo(t):
    # Two soft, breathy hoots, falling slightly: a nod to the name.
    out = 0.0
    for start, f, dur in ((0.0, 520.0, 0.22), (0.26, 470.0, 0.34)):
        u = t - start
        if 0 <= u <= dur:
            shape = math.sin(math.pi * u / dur) ** 1.6
            vib = 1 + 0.012 * math.sin(2 * math.pi * 6 * u)
            out += shape * (math.sin(2 * math.pi * f * vib * u) + 0.22 * math.sin(2 * math.pi * 2 * f * u))
    return out


def chime(t):
    return bell(1318.5, 0.9, 0.22)(t) * 0.8 + bell(1975.5, 0.9, 0.28, start=0.11)(t) * 0.6


def bowl(t):
    return bell(784.0, 1.2, 0.42, partials=((1, 1.0), (2.0, 0.08), (2.98, 0.05)), attack=0.012)(t)


def wood(t):
    # Soft marimba-ish knock, two quick notes.
    p = ((1, 1.0), (3.9, 0.12))
    return bell(988.0, 0.35, 0.06, partials=p)(t) + bell(1318.5, 0.35, 0.07, partials=p, start=0.09)(t) * 0.8


def droplet_meeting(t):
    # Three drops, each a little higher: same timbre, reads as "time to go".
    out = 0.0
    for i, start in enumerate((0.0, 0.16, 0.32)):
        u = t - start
        if 0 <= u <= 0.32:
            f0, f1, k = 900.0 * 1.19 ** i, 560.0 * 1.19 ** i, 18.0
            phase = 2 * math.pi * (f1 * u + (f0 - f1) * (1 - math.exp(-k * u)) / k)
            out += env(u, 0.003, 0.07) * math.sin(phase)
    return out


def coo_meeting(t):
    # Three hoots, the last one rising.
    out = 0.0
    for start, f0, f1, dur in ((0.0, 470.0, 470.0, 0.2), (0.24, 470.0, 470.0, 0.2), (0.48, 470.0, 560.0, 0.34)):
        u = t - start
        if 0 <= u <= dur:
            shape = math.sin(math.pi * u / dur) ** 1.6
            f = f0 + (f1 - f0) * (u / dur)
            out += shape * (math.sin(2 * math.pi * f * u) + 0.22 * math.sin(2 * math.pi * 2 * f * u))
    return out


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
    "droplet": ((droplet, 0.35), (droplet_meeting, 0.7)),
    "coo": ((coo, 0.65), (coo_meeting, 0.85)),
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
