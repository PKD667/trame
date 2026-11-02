"""CPU reference for case 2: 100 keys, 300 items, order-dependent fold. Independent of the Rust code.

Inputs (as in gpu/src/main.rs, case 2): item i in 0..300 has key (37*i) % 100 and value 1 + 13*i.
Items are folded per key in input order: acc = (acc*3 + value + 5) % 1000003; seen[value % 2] += 1.
branch = 1 if seen[0] > seen[1] else 2. Output: the host's stdout format, one line per slot, then the outcome.
"""
import sys
keys = 100
acc = [0] * keys
seen = [[0, 0] for _ in range(keys)]
for i in range(300):
    k = (37 * i) % 100
    v = 1 + 13 * i
    acc[k] = (acc[k] * 3 + v + 5) % 1000003
    seen[k][v % 2] += 1
out = []
for k in range(keys):
    branch = 1 if seen[k][0] > seen[k][1] else 2
    out.append(f"slot {k} acc {acc[k]} branch {branch}")
out.append("outcomes: [Returned]")
sys.stdout.write("\n".join(out) + "\n")
