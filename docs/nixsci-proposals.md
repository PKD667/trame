# Proposed changes to nixsci, from using it for trame

nixsci moves too, so where it did not fit, these are changes to propose to it, not workarounds to
keep in trame. Each says what happened, what to change, and what would show the change works.

## 1. Let the target substitute what a public cache has

What happened (2026-10-31, gros-101, Nancy). `nixsci deploy shell` of trame's `deployments.dev`
(rustc, cargo, clang, Open MPI: 157 paths, 3.4 GiB) pushed the whole closure from the controller, a
laptop, over its uplink through the Grid'5000 access host. It failed after 73 min with `scp:
Couldn't send packet: Connection reset by peer`. The same runtime paths, substituted by the node
itself from cache.nixos.org with nixsci's static nix (`nix --store /tmp/pkronlun-nix/store build
--no-link <paths>`, Nix 2.34.8), arrived in 34 s: 156 paths, 3.5 GB on disk.

Change. The controller sends the closure's path list and NAR hashes. The target first substitutes
from the configured public caches (signatures checked by Nix), then the controller pushes only the
paths still missing (here: the deployment's own wrapper), and the target compares every NAR hash, as
it does now. Verification stays the same; the controller's uplink is no longer on the data path.
A target with no outbound network keeps today's push.

Shown to work when: the same `deploy shell` on gros-101 stages in under a minute, and the logs list
the pushed paths (expected: one, the wrapper).

## 2. `deploy shell` of a dev shell

What happened. The environment wanted on the node is `devShells.x86_64-linux.mpi`, which trame
already defines. `deploy shell` takes only `deployments.<system>.<name>`, so trame repeats the dev
shell as a deployment (`deployments.dev` in flake.nix), with the same packages and env, by hand.

Change. `nixsci deploy shell --devshell FLAKE#name` builds the dev shell's environment (its
`inputDerivation` and the env its `shellHook` exports) and enters it, with the same closure checks.

## 3. Cleanliness only for what goes into the closure

What happened. `deploy shell` refuses a dirty tree. For `run` that is right: the closure is the
experiment. For `shell` during development, the closure is only the environment, and the source under
test is dirty by nature; trame ships it separately (a git bundle plus the diff, g5k/sync.sh), which
nixsci could do itself.

Change. `shell` checks cleanliness of the inputs of the closure only; `--source DIR` ships a working
tree (HEAD bundle, diff, untracked files) into the workdir, recording the commit and the diff's hash.

## 4. Do not share ssh control sockets between users of one account

What happened. The shared `~/.cache/cluster-ssh` master (another agent's) hung on the node hop
("Connection timed out during banner exchange"); trame's targets file then used its own ssh config
with its own ControlPath, which connected in 2 s. Grid'5000 counts logins, so multiplexing is needed;
sharing one master between independent sessions makes one session's stale socket another's outage.

Change. Per-lease control paths by default (`ControlPath <lease dir>/ssh-%C`), and a check that a
master answers (`ssh -O check`) before reusing it.
