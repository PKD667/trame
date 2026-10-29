---- MODULE Epoch ----
\* The wait protocol of one worker or the leader: backends/mpi/epoch.rs (sleep, sleep_on, bump_word, epoch) and the
\* futex calls of backends/mpi/futex.rs (wait_for compares and sleeps atomically; wake wakes sleepers).
\* Interleaving semantics: every access is SeqCst (epoch.rs lines 1-16), so one total order.
\* One waiter, one bumper that acts K times. W is the modulus of the 32-bit epoch word.
\* Not modelled: timeouts (until = None), the relay and leader words (same protocol), Relaxed stores
\* other than the one at epoch.rs line 113 (see proofs/README.md).
EXTENDS Naturals

CONSTANTS W, K

VARIABLES
  E,      \* the epoch word, 0..W-1
  sleep,  \* the sleep flag word, 0 or 1
  cond,   \* the waiter's condition: a frame is there
  seen,   \* the waiter's local copy of E (its `seen`)
  pcw,    \* waiter's program counter
  pcb,    \* bumper's program counter
  bumps   \* completed bumps, 0..K

vars == <<E, sleep, cond, seen, pcw, pcb, bumps>>

\* Waiter program counters, with the epoch.rs line each one is:
\*  r1     seen := E                     (epoch.rs 40, the caller's read)
\*  r2     if cond then done else r2b    (the caller's condition check)
\*  r2b    if E # seen then r1 else r3   (epoch.rs 103, the loop's first test)
\*  r3     sleep := 1                    (epoch.rs 110)
\*  r4     if E # seen then sleep:=0; r1 (epoch.rs 112-115, the Dekker re-read)
\*  kern   FUTEX_WAIT: if E = seen then fw, else EAGAIN (futex.rs 24-27), atomic
\*  fw     blocked in the kernel
\*  woken  sleep := 0; r2b                (epoch.rs 117, after wait_for returns)
\* Bumper program counters:
\*  b1     cond := TRUE                  (the act: epoch.rs 44-47 caller's frame store)
\*  b2     E := E + 1 mod W; bumps += 1  (epoch.rs 51, fetch_add)
\*  b3     if sleep = 1 then FUTEX_WAKE  (epoch.rs 52-53: wakes a waiter in fw)

Init ==
  /\ E = 0 /\ sleep = 0 /\ cond = FALSE /\ seen = 0
  /\ pcw = "r1" /\ pcb = "b1" /\ bumps = 0

W1 == /\ pcw = "r1"
      /\ seen' = E
      /\ pcw' = "r2"
      /\ UNCHANGED <<E, sleep, cond, pcb, bumps>>

W2 == /\ pcw = "r2"
      /\ pcw' = IF cond THEN "done" ELSE "r2b"
      /\ UNCHANGED <<E, sleep, cond, seen, pcb, bumps>>

W2b == /\ pcw = "r2b"
       /\ pcw' = IF E # seen THEN "r1" ELSE "r3"
       /\ UNCHANGED <<E, sleep, cond, seen, pcb, bumps>>

W3 == /\ pcw = "r3"
      /\ sleep' = 1
      /\ pcw' = "r4"
      /\ UNCHANGED <<E, cond, seen, pcb, bumps>>

W4 == /\ pcw = "r4"
      /\ IF E # seen
           THEN /\ sleep' = 0 /\ pcw' = "r1"
           ELSE /\ sleep' = sleep /\ pcw' = "kern"
      /\ UNCHANGED <<E, cond, seen, pcb, bumps>>

\* FUTEX_WAIT: the compare and the sleep are one step.
Kern == /\ pcw = "kern"
        /\ IF E = seen
             THEN /\ pcw' = "fw" /\ sleep' = sleep
             ELSE /\ pcw' = "r2b" /\ sleep' = 0
        /\ UNCHANGED <<E, cond, seen, pcb, bumps>>

\* FUTEX_WAKE by the bumper reaches a waiter that is in the kernel.
Woken == /\ pcw = "woken"
         /\ pcw' = "r2b" /\ sleep' = 0
         /\ UNCHANGED <<E, cond, seen, pcb, bumps>>

WaiterStep == W1 \/ W2 \/ W2b \/ W3 \/ W4 \/ Kern \/ Woken

B1 == /\ pcb = "b1"
      /\ cond' = TRUE
      /\ pcb' = "b2"
      /\ UNCHANGED <<E, sleep, seen, pcw, bumps>>

B2 == /\ pcb = "b2"
      /\ E' = (E + 1) % W
      /\ bumps' = bumps + 1
      /\ pcb' = "b3"
      /\ UNCHANGED <<sleep, cond, seen, pcw>>

B3 == /\ pcb = "b3"
      /\ pcw' = IF sleep = 1 /\ pcw = "fw" THEN "woken" ELSE pcw
      /\ pcb' = IF bumps < K THEN "b1" ELSE "done"
      /\ UNCHANGED <<E, sleep, cond, seen, bumps>>

BumperStep == B1 \/ B2 \/ B3

Next == WaiterStep \/ BumperStep \/ (pcw = "done" /\ pcb = "done" /\ UNCHANGED vars)

\* Weak fairness of each process's steps: a step that stays enabled is eventually taken.
Spec == Init /\ [][Next]_vars /\ WF_vars(WaiterStep) /\ WF_vars(BumperStep)

TypeOK ==
  /\ E \in 0..(W-1) /\ sleep \in {0, 1} /\ cond \in BOOLEAN /\ seen \in 0..(W-1)
  /\ pcw \in {"r1", "r2", "r2b", "r3", "r4", "kern", "fw", "woken", "done"}
  /\ pcb \in {"b1", "b2", "b3", "done"} /\ bumps \in 0..K

\* Property (a). A waiter asleep in the kernel, with its condition true, and no bumper left to wake it.
NoLostWakeup == ~(pcw = "fw" /\ cond /\ pcb = "done")

\* Property (b). Once the condition is true, the waiter eventually finishes.
Live == cond ~> (pcw = "done")

====
