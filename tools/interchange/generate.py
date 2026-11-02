#!/usr/bin/env python3
"""Writes a Norn long-format CSV: SIGNALS signals at 1 Hz for SECONDS seconds.

A hand-written file in the documented form (docs/reference/adapters.md), with a
repeatable mix of signal shapes: 60% noisy double sensors, 20% float setpoints that
step, 10% booleans, 10% slow int32 counters. The shapes are assumed, not from a site.

usage: generate.py SIGNALS SECONDS > data.csv
"""
import math
import random
import sys

signals, seconds = int(sys.argv[1]), int(sys.argv[2])
random.seed(1)
START = 1_793_577_600  # 2026-11-02T00:00:00Z
print("time,signal,stream,type,value,severity,substatus,sequence")


def stamp(t):
    days = t // 86_400 - 20_759
    return f"2026-11-{days + 2:02d}T{t % 86_400 // 3600:02d}:{t % 3600 // 60:02d}:{t % 60:02d}Z"


for s in range(signals):
    entity = f"ahu-{s // 10:03d}"
    if s % 10 < 6:
        kind, name = "double", f"temp-{s}"
    elif s % 10 < 8:
        kind, name = "float", f"setpoint-{s}"
    elif s % 10 == 8:
        kind, name = "bool", f"fan-{s}"
    else:
        kind, name = "int32", f"count-{s}"
    signal = f"entities/{entity}/signals/{name}"
    state = 0
    for i in range(seconds):
        if kind == "double":
            value = round(20 + 5 * math.sin(i / 600 + s) + random.gauss(0, 0.05), 3)
        elif kind == "float":
            value = 21.5 + (i // 1800 + s) % 4
        elif kind == "bool":
            value = "true" if (i // 900 + s) % 3 else "false"
        else:
            state += random.random() < 0.2
            value = state
        print(f"{stamp(START + i)},{signal},streams/plant,{kind},{value},GOOD,,{i}")
