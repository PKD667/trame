---- MODULE Epoch ----
\* The wait protocol of one worker rank or the leader: src/backends/mpi/waiting/epoch.rs (sleep_on, bump_word, epoch) and the
\* futex calls of src/backends/mpi/waiting/futex.rs (wait_for compares and sleeps atomically; wake wakes every sleeper).
\* Interleaving semantics: every access is SeqCst (epoch.rs lines 1-16), so one total order.
\* N sleepers (the agents of one rank; worker.rs runs k of them) on one epoch word, one bumper that acts K times.
\* W is the modulus of the 32-bit epoch word.
\* SLEEP is a count of the agents between their fetch_add and their fetch_sub (COUNTED = TRUE, the code now), or
\* the flag of the earlier code (COUNTED = FALSE: store 1 before the sleep, store 0 after it), kept to show that
\* the flag loses a wake-up at N >= 2 (counterexamples/epoch-flag-k2.txt).
\* LOCKED models XPORT (roles/worker.rs 39): an agent takes it before sleep_on and holds it until sleep_on returns, so
\* at most one agent of a rank is inside sleep_on. LOCKED = FALSE is the word's protocol alone, which is what
\* epoch.rs claims and what the leader's word sees; LOCKED = TRUE is the worker rank as it runs.
\* Not modelled: timeouts (until = None), the leader's word (same protocol), weaker
\* orderings (see check/proofs/README.md), spurious futex returns.
EXTENDS Naturals, FiniteSets

CONSTANTS W, K, N, COUNTED, LOCKED

Sl == 1..N

VARIABLES
  E,      \* the epoch word, 0..W-1
  sleep,  \* the SLEEP word: a count 0..N (COUNTED) or a flag 0..1
  cond,   \* the sleepers' condition: a frame is there
  seen,   \* each sleeper's local copy of E (its `seen`)
  pcw,    \* each sleeper's program counter
  pcb,    \* bumper's program counter
  bumps,  \* completed bumps, 0..K
  ld,     \* the bumper's loaded SLEEP != 0
  lock    \* XPORT: 0 free, else the sleeper that holds it (only if LOCKED)

vars == <<E, sleep, cond, seen, pcw, pcb, bumps, ld, lock>>

\* Sleeper program counters, with the epoch.rs line each one is:
\*  r1     seen := E                     (epoch.rs 42, the caller's read)
\*  r2     if cond then done else acq    (the caller's condition check)
\*  acq    take XPORT                    (worker.rs 155, `idle`; a no-op unless LOCKED)
\*  r2b    if E # seen then r1 else r3   (epoch.rs 104, the loop's first test; r1 releases XPORT)
\*  r3     SLEEP += 1 (flag: := 1)       (epoch.rs 113, fetch_add)
\*  r4     if E # seen then SLEEP -= 1; r1 (epoch.rs 117-118, the Dekker re-read)
\*  kern   FUTEX_WAIT: if E = seen then fw, else EAGAIN (futex.rs 24-27), atomic
\*  fw     blocked in the kernel
\*  woken  returned from the kernel
\*  Both kern (EAGAIN) and woken go to dec, which is SLEEP -= 1 (flag: := 0), epoch.rs 122, then r2b.
\* Bumper program counters:
\*  b1     cond := TRUE                  (the act: the caller's frame store)
\*  b2     E := E + 1 mod W; bumps += 1  (epoch.rs 52, fetch_add)
\*  b3     ld := SLEEP # 0               (epoch.rs 53)
\*  b4     if ld then FUTEX_WAKE         (epoch.rs 54: wakes every sleeper in fw at that time)
\* Line numbers are those of src/backends/mpi/waiting/epoch.rs at the commit that adds this model.

Inc == IF COUNTED THEN sleep + 1 ELSE 1
Dec == IF COUNTED THEN sleep - 1 ELSE 0

Init ==
  /\ E = 0 /\ sleep = 0 /\ cond = FALSE
  /\ seen = [i \in Sl |-> 0] /\ pcw = [i \in Sl |-> "r1"]
  /\ pcb = "b1" /\ bumps = 0 /\ ld = FALSE /\ lock = 0

W1(i) == /\ pcw[i] = "r1"
         /\ seen' = [seen EXCEPT ![i] = E]
         /\ pcw' = [pcw EXCEPT ![i] = "r2"]
         /\ UNCHANGED <<E, sleep, cond, pcb, bumps, ld, lock>>

W2(i) == /\ pcw[i] = "r2"
         /\ pcw' = [pcw EXCEPT ![i] = IF cond THEN "done" ELSE "acq"]
         /\ UNCHANGED <<E, sleep, cond, seen, pcb, bumps, ld, lock>>

Acq(i) == /\ pcw[i] = "acq"
          /\ lock = 0
          /\ lock' = IF LOCKED THEN i ELSE 0
          /\ pcw' = [pcw EXCEPT ![i] = "r2b"]
          /\ UNCHANGED <<E, sleep, cond, seen, pcb, bumps, ld>>

W2b(i) == /\ pcw[i] = "r2b"
          /\ IF E # seen[i]
               THEN /\ pcw' = [pcw EXCEPT ![i] = "r1"] /\ lock' = 0
               ELSE /\ pcw' = [pcw EXCEPT ![i] = "r3"] /\ lock' = lock
          /\ UNCHANGED <<E, sleep, cond, seen, pcb, bumps, ld>>

W3(i) == /\ pcw[i] = "r3"
         /\ sleep' = Inc
         /\ pcw' = [pcw EXCEPT ![i] = "r4"]
         /\ UNCHANGED <<E, cond, seen, pcb, bumps, ld, lock>>

W4(i) == /\ pcw[i] = "r4"
         /\ IF E # seen[i]
              THEN /\ sleep' = Dec /\ pcw' = [pcw EXCEPT ![i] = "r1"] /\ lock' = 0
              ELSE /\ sleep' = sleep /\ pcw' = [pcw EXCEPT ![i] = "kern"] /\ lock' = lock
         /\ UNCHANGED <<E, cond, seen, pcb, bumps, ld>>

\* FUTEX_WAIT: the compare and the sleep are one step.
Kern(i) == /\ pcw[i] = "kern"
           /\ pcw' = [pcw EXCEPT ![i] = IF E = seen[i] THEN "fw" ELSE "dec"]
           /\ UNCHANGED <<E, sleep, cond, seen, pcb, bumps, ld, lock>>

\* The bumper's FUTEX_WAKE reached this sleeper in the kernel (B4).
Woken(i) == /\ pcw[i] = "woken"
            /\ pcw' = [pcw EXCEPT ![i] = "dec"]
            /\ UNCHANGED <<E, sleep, cond, seen, pcb, bumps, ld, lock>>

Dec1(i) == /\ pcw[i] = "dec"
           /\ sleep' = Dec
           /\ pcw' = [pcw EXCEPT ![i] = "r2b"]
           /\ UNCHANGED <<E, cond, seen, pcb, bumps, ld, lock>>

SleeperStep(i) == W1(i) \/ W2(i) \/ Acq(i) \/ W2b(i) \/ W3(i) \/ W4(i) \/ Kern(i) \/ Woken(i) \/ Dec1(i)

B1 == /\ pcb = "b1"
      /\ cond' = TRUE
      /\ pcb' = "b2"
      /\ UNCHANGED <<E, sleep, seen, pcw, bumps, ld, lock>>

B2 == /\ pcb = "b2"
      /\ E' = (E + 1) % W
      /\ bumps' = bumps + 1
      /\ pcb' = "b3"
      /\ UNCHANGED <<sleep, cond, seen, pcw, ld, lock>>

B3 == /\ pcb = "b3"
      /\ ld' = (sleep # 0)
      /\ pcb' = "b4"
      /\ UNCHANGED <<E, sleep, cond, seen, pcw, bumps, lock>>

B4 == /\ pcb = "b4"
      /\ pcw' = [i \in Sl |-> IF ld /\ pcw[i] = "fw" THEN "woken" ELSE pcw[i]]
      /\ pcb' = IF bumps < K THEN "b1" ELSE "done"
      /\ UNCHANGED <<E, sleep, cond, seen, bumps, ld, lock>>

BumperStep == B1 \/ B2 \/ B3 \/ B4

AllDone == \A i \in Sl : pcw[i] = "done"

Next == (\E i \in Sl : SleeperStep(i)) \/ BumperStep \/ (AllDone /\ pcb = "done" /\ UNCHANGED vars)

\* Weak fairness of each process's steps: a step that stays enabled is eventually taken.
Spec == Init /\ [][Next]_vars /\ (\A i \in Sl : WF_vars(SleeperStep(i))) /\ WF_vars(BumperStep)

TypeOK ==
  /\ E \in 0..(W-1) /\ sleep \in 0..(IF COUNTED THEN N ELSE 1) /\ cond \in BOOLEAN
  /\ seen \in [Sl -> 0..(W-1)]
  /\ pcw \in [Sl -> {"r1", "r2", "acq", "r2b", "r3", "r4", "kern", "fw", "woken", "dec", "done"}]
  /\ pcb \in {"b1", "b2", "b3", "b4", "done"} /\ bumps \in 0..K /\ ld \in BOOLEAN
  /\ lock \in 0..N /\ (~LOCKED => lock = 0)

\* Property (a). A sleeper asleep in the kernel, with its condition true, and no bumper left to wake it.
NoLostWakeup == \A i \in Sl : ~(pcw[i] = "fw" /\ cond /\ pcb = "done")

\* Property (b). Once the condition is true, every sleeper eventually finishes.
Live == \A i \in Sl : cond ~> (pcw[i] = "done")

\* Property (c). SLEEP announces every sleeper that has added and not yet subtracted: it is not zero while one
\* is in r4, kern, fw, woken or dec. This is what `bump_word` relies on (epoch.rs 53). The flag breaks it as soon as
\* one sleeper clears the flag while another sleeps (N >= 2), which is the cause of the lost wake-up.
In == {i \in Sl : pcw[i] \in {"r4", "kern", "fw", "woken", "dec"}}
Announced == (In # {}) => (sleep # 0)

\* Property (d). The count is exact: it equals the number of sleepers that have added and not subtracted.
CountExact == COUNTED => sleep = Cardinality(In)

====
