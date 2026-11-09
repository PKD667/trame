"""Summarise one credit-return run directory written by progress.c.

usage: python3 analyze.py DIR CAP
Latency of a credit = sender's observation time (S line) minus the receiver's ack-return time
(R line of the consumption that freed the slot). Times are CLOCK_MONOTONIC ns on one machine.
"""
import glob, os, statistics, sys

d, cap = sys.argv[1], int(sys.argv[2])
sender = glob.glob(os.path.join(d, "*.sender"))[0]
receiver = glob.glob(os.path.join(d, "*.receiver"))[0]

R = {}
for line in open(receiver):
    p = line.split()
    if p[0] == "R":
        R[int(p[1])] = int(p[2])
corrupt = sum(1 for l in open(receiver) if l.startswith("CORRUPT"))

fulls, bad_full, stalls, lat, neg, stalled_waits = 0, 0, 0, [], 0, 0
for line in open(sender):
    p = line.split()
    if p[0] == "F":
        fulls += 1
        sent, acked = int(p[3]), int(p[4])
        if sent - acked != cap:
            bad_full += 1  # Full answered without cap frames outstanding
    elif p[0] == "P":
        stalls += 1
    elif p[0] == "S":
        k, t, acked, stalled = int(p[1]), int(p[2]), int(p[3]), int(p[4])
        stalled_waits += stalled
        j = k - cap
        if j >= 1 and j in R:
            v = t - R[j]
            lat.append(v)
            neg += v < 0

print(f"dir={os.path.basename(d)} full_answers={fulls} full_without_cap_outstanding={bad_full}"
      f" stall_events={stalls} waits_that_stalled={stalled_waits} corrupt={corrupt}")
if lat:
    lat.sort()
    q = lambda f: lat[min(len(lat) - 1, int(f * len(lat)))]
    print(f"  credit latency ns: n={len(lat)} min={lat[0]} median={int(statistics.median(lat))}"
          f" p90={q(0.90)} p99={q(0.99)} max={lat[-1]} negative={neg}")
else:
    print("  no credit latency samples")
