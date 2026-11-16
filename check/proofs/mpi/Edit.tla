---- MODULE Edit ----
\* The leader's edit handshake of one worker, on one node: backends/mpi/roles/leader.rs `edit` (lines 116-153), the slot
\* scheduler's `remote_edit` and `agent` (src/worker.rs 484-504, 569-629), backends/mpi/roles/worker.rs `edit_request` (207),
\* `edit_held` (214), `idle` (240), and the wait protocol of backends/mpi/waiting/epoch.rs (`bump_word` 48, `sleep_on` 96,
\* `leader_wait` 73); `finish` (roles/results.rs 35-43). Contract clauses S1, S3, S4, P2 (a pool edit is this handshake once per worker; the model has one
\* worker), M5 (what f wrote is visible to steps that start after the release: not modelled, see below).
\* Interleaving semantics, every access SeqCst, as in Epoch.tla.
\* N agents of one worker rank share the slot mutex (`slk`), the transport lock XPORT (`xp`) and the worker's epoch
\* word `EW` with its sleep count `SW`. The leader sleeps on its own epoch `EL` with count `SL`.
\* A step is abstracted by the set of fields it touches (`run[i]`); Fields is the set of fields an edit may hold
\* (the STATE bit and the POOL bit in the code); a frame of kind T makes a step touching T wanted.
\* Plan is the sequence of field sets the leader edits, one edit after the other.
\* Epoch protocol: Epoch.tla is not instanced; the same steps are re-encoded here for two words, with the sleeper
\* count and the atomic futex compare-and-sleep. The Dekker re-read after the count increment (epoch.rs 111) is in
\* both waiters: the leader's at W5, the worker's at Dk. Epoch.tla leaves it out; the same sleep_on is used on both sides.
\* The epoch words are not reduced modulo 2^32 here (EpochWrap in Epoch.tla covers the wrap).
\* XACK = TRUE is the deliberately broken variant in which `edit_held` takes XPORT (as `with_endpoint` does); the
\* code does not (backends/mpi/roles/worker.rs `edit_held`, 214-222). WAITREL = FALSE is the broken variant in which the leader
\* does not wait for HELD = 0 before the next edit; the code waits (leader.rs 150).
\* Not modelled: timeouts of the idle sleep, spurious futex returns, steps' own transport calls that need XPORT,
\* several workers (a pool edit repeats the handshake per worker and waits for all), stop, failure, abort,
\* the leader of another node, the memory the edit writes (M5), steps that wait on deadlines or room.
\* The worker's result: an agent that ends a step with no other running may find the worker answered (`ans`); every
\* agent then returns, and after the last one `finish` stores the result and bumps the leader (roles/runner.rs 50-64).
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS N, Fields, Plan, F, XACK, WAITREL

Sl == 1..N
Kinds == {{}} \cup {{f} : f \in Fields}
NONE == {0}  \* no step: a set, because TLC does not compare a string with a set; 0 is not a field
LEAD == N + 1
ENV == N + 2
FIN == N + 3
Bumpers == Sl \cup {LEAD, ENV, FIN}

VARIABLES
  EW, SW,   \* worker epoch word, its sleep count (agents' sleep_on)
  EL, SL,   \* leader epoch word, its sleep count
  edit,     \* the EDIT word: fields the leader asks the worker to hold, {} if none
  held,     \* the HELD word, 0 or 1
  res,      \* the worker's outcome word is not 0
  xp,       \* XPORT: 0 free, else the agent holding it
  slk,      \* the slot mutex: 0 free, else the agent holding it
  editing,  \* Sched.editing: fields no step touching them may start on
  remote,   \* Inner.remote: the fields held for the leader, {} if none
  acked,    \* Inner.acked
  run,      \* run[i]: the field set of the step agent i runs, or NONE
  ans,      \* Sched.answer() is Some: no step starts, every agent returns
  pq,       \* kinds of the frames waiting for a step (the transport's frames)
  sent,     \* frames sent so far
  pcw,      \* each agent's program counter
  seen,     \* each agent's epoch read at the top of its loop (`worker_epoch`)
  have,     \* the kinds an agent saw offered when it read the conditions (before the lock)
  rq,       \* the EDIT value an agent read in `remote_edit`
  ld,       \* each bumper's loaded `sleep != 0`
  pcl,      \* leader's program counter
  k,        \* index in Plan of the edit the leader is on
  cur,      \* fields of that edit
  sl,       \* leader's epoch read (`leader_epoch`)
  ph,       \* what the leader waits for: 1 HELD = 1, 2 HELD = 0
  runit,    \* the leader's `run_it`
  fruns,    \* number of times f ran, per edit
  nret,     \* edits that returned
  pcn,      \* the frame sender's program counter (the environment: other workers, the relay)
  pcz       \* the worker's `finish` program counter

vars == <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq,
          ld, pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

\* Agent program counters, with the code each one is:
\*  top     seen := EW                                   (worker.rs 195, `epoch`; agent loop top, worker.rs 602)
\*  snap    have := kinds of the frames offered          (`conditions`, read without the slot lock)
\*  lock    take the slot mutex                          (worker.rs 614 `slot.lock()`)
\*  body    if ans then return; rq := EDIT; hold         (`g.s.answer()`, `remote_edit` first branch)
\*  chk1    if remote, not acked, no running step touches it: ack      (`remote_edit` second branch)
\*  a0..a5  ack: [take XPORT if XACK]; HELD := 1; EL += 1; ld := SL != 0; wake the leader if ld; acked := TRUE
\*  chk2    if remote, acked, rq = {}: release           (`remote_edit` third branch)
\*  q0..q5  release: [take XPORT]; HELD := 0; EL += 1; ld; wake; notify the slot's condvar
\*  choose  start a step the model admits, or wait: condvar if a step runs, else idle  (`choose`, worker.rs 625)
\*  run     the step runs; its End takes the slot mutex, notifies the condvar          (worker.rs 639)
\*  cv      Condvar::wait                                (worker.rs 650)
\*  ia      take XPORT                                   (`idle`, worker.rs 205)
\*  s1      if EW # seen then release XPORT, go to top   (epoch.rs 104, the loop test)
\*  s2      SW += 1                                      (epoch.rs 107)
\*  dk      re-read EW: if EW # seen then SW -= 1, release XPORT, go to top; else kern   (epoch.rs 111-112)
\*  kern    FUTEX_WAIT: if EW = seen then fw else EAGAIN (futex.rs 24-27), atomic
\*  fw      blocked in the kernel; woken is the kernel's return
\*  dec     SW -= 1, back to s1                          (epoch.rs 122)
\*  exit    returned from `agent`
\* Leader program counters:
\*  l0      next edit of Plan or done
\*  e1      EDIT := cur                                  (leader.rs 141)
\*  e2,e3,e4 bump(w): EW += 1; ld := SW != 0; wake       (epoch.rs 50-55)
\*  w1..w7  wait_all: sl := EL; if done then out else leader_wait: loop {if EL # sl return; SL += 1; (W5) re-read EL; (W6) kern: if EL = sl then sleep (lfw) else EAGAIN; SL -= 1}
\*  x1      runit := not res                             (leader.rs 145-148)
\*  f1,f2   f runs (f2 is the interval in which f runs)  (leader.rs 150)
\*  d1..d4  EDIT := {}; bump                             (leader.rs 153)
\*  nx      the edit returned
\* The frame sender: pcn n0 sends a frame of some kind, then n1..n3 bump the worker's epoch (a Transport::send).
\* The finish: z0 waits for every agent to return; z1 stores the result; z2..z4 bump the leader's epoch.

Init ==
  /\ EW = 0 /\ SW = 0 /\ EL = 0 /\ SL = 0
  /\ edit = {} /\ held = 0 /\ res = FALSE /\ xp = 0 /\ slk = 0
  /\ editing = {} /\ remote = {} /\ acked = FALSE
  /\ run = [i \in Sl |-> NONE] /\ ans = FALSE /\ pq = {} /\ sent = 0
  /\ pcw = [i \in Sl |-> "top"] /\ seen = [i \in Sl |-> 0] /\ have = [i \in Sl |-> {}] /\ rq = [i \in Sl |-> {}]
  /\ ld = [b \in Bumpers |-> FALSE]
  /\ pcl = "l0" /\ k = 1 /\ cur = {} /\ sl = 0 /\ ph = 1 /\ runit = FALSE
  /\ fruns = [m \in 1..Len(Plan) |-> 0] /\ nret = 0
  /\ pcn = "n0" /\ pcz = "z0"

Touching(S) == \E j \in Sl : run[j] # NONE /\ run[j] \cap S # {}
Running == \E j \in Sl : run[j] # NONE
Allowed == {T \in Kinds : T \cap editing = {}}

\* A futex wake of the worker's epoch: every agent in the kernel at that moment returns, if the bumper loaded a non-zero count.
WakeW(l) == [j \in Sl |-> IF l /\ pcw[j] = "fw" THEN "woken" ELSE pcw[j]]
WakeL(l) == IF l /\ pcl = "lfw" THEN "w7" ELSE pcl
\* The slot's condvar: every agent waiting on it goes to the top of its loop; agent i takes `p`.
Notify(i, p) == [j \in Sl |-> IF j = i THEN p ELSE IF pcw[j] = "cv" THEN "top" ELSE pcw[j]]

\* ---- agents ----

Top(i) == /\ pcw[i] = "top"
          /\ seen' = [seen EXCEPT ![i] = EW]
          /\ pcw' = [pcw EXCEPT ![i] = "snap"]
          /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, have, rq, ld,
                         pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Snap(i) == /\ pcw[i] = "snap"
           /\ have' = [have EXCEPT ![i] = pq]
           /\ pcw' = [pcw EXCEPT ![i] = "lock"]
           /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, rq, ld,
                          pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Lock(i) == /\ pcw[i] = "lock" /\ slk = 0
           /\ slk' = i
           /\ pcw' = [pcw EXCEPT ![i] = "body"]
           /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                          pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

\* The answer is checked first: a returning agent does not look at the request.
Body(i) == /\ pcw[i] = "body"
           /\ IF ans
                THEN /\ slk' = 0
                     /\ pcw' = Notify(i, "exit")
                     /\ UNCHANGED <<editing, remote, acked, rq>>
                ELSE /\ rq' = [rq EXCEPT ![i] = edit]
                     /\ IF edit # {} /\ remote = {}
                          THEN /\ editing' = editing \cup edit /\ remote' = edit /\ acked' = FALSE
                          ELSE UNCHANGED <<editing, remote, acked>>
                     /\ pcw' = [pcw EXCEPT ![i] = "chk1"]
                     /\ UNCHANGED slk
           /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, run, ans, pq, sent, seen, have, ld,
                          pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Chk1(i) == /\ pcw[i] = "chk1"
           /\ pcw' = [pcw EXCEPT ![i] = IF remote # {} /\ ~acked /\ ~Touching(remote) THEN (IF XACK THEN "a0" ELSE "a1") ELSE "chk2"]
           /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                          pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

A0(i) == /\ pcw[i] = "a0" /\ xp = 0
         /\ xp' = i
         /\ pcw' = [pcw EXCEPT ![i] = "a1"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

A1(i) == /\ pcw[i] = "a1"
         /\ held' = 1
         /\ pcw' = [pcw EXCEPT ![i] = "a2"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

A2(i) == /\ pcw[i] = "a2"
         /\ EL' = EL + 1
         /\ pcw' = [pcw EXCEPT ![i] = "a3"]
         /\ UNCHANGED <<EW, SW, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

A3(i) == /\ pcw[i] = "a3"
         /\ ld' = [ld EXCEPT ![i] = (SL # 0)]
         /\ pcw' = [pcw EXCEPT ![i] = "a4"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

A4(i) == /\ pcw[i] = "a4"
         /\ pcl' = WakeL(ld[i])
         /\ xp' = IF XACK THEN 0 ELSE xp
         /\ pcw' = [pcw EXCEPT ![i] = "a5"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

A5(i) == /\ pcw[i] = "a5"
         /\ acked' = TRUE
         /\ pcw' = [pcw EXCEPT ![i] = "chk2"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Chk2(i) == /\ pcw[i] = "chk2"
           /\ IF remote # {} /\ acked /\ rq[i] = {}
                THEN /\ editing' = editing \ remote /\ remote' = {} /\ acked' = FALSE
                     /\ pcw' = [pcw EXCEPT ![i] = IF XACK THEN "q0" ELSE "q1"]
                ELSE /\ pcw' = [pcw EXCEPT ![i] = "choose"]
                     /\ UNCHANGED <<editing, remote, acked>>
           /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, run, ans, pq, sent, seen, have, rq, ld,
                          pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Q0(i) == /\ pcw[i] = "q0" /\ xp = 0
         /\ xp' = i
         /\ pcw' = [pcw EXCEPT ![i] = "q1"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Q1(i) == /\ pcw[i] = "q1"
         /\ held' = 0
         /\ pcw' = [pcw EXCEPT ![i] = "q2"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Q2(i) == /\ pcw[i] = "q2"
         /\ EL' = EL + 1
         /\ pcw' = [pcw EXCEPT ![i] = "q3"]
         /\ UNCHANGED <<EW, SW, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Q3(i) == /\ pcw[i] = "q3"
         /\ ld' = [ld EXCEPT ![i] = (SL # 0)]
         /\ pcw' = [pcw EXCEPT ![i] = "q4"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Q4(i) == /\ pcw[i] = "q4"
         /\ pcl' = WakeL(ld[i])
         /\ xp' = IF XACK THEN 0 ELSE xp
         /\ pcw' = [pcw EXCEPT ![i] = "q5"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

\* `slot.changed.notify_all()` after the release; the agent goes on to choose, still holding the slot mutex.
Q5(i) == /\ pcw[i] = "q5"
         /\ pcw' = Notify(i, "choose")
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

\* choose: some step the model admits whose frame was offered starts; the slot mutex is dropped.
Start(i, T) == /\ pcw[i] = "choose"
               /\ T \in have[i] /\ T \in Allowed
               /\ run' = [run EXCEPT ![i] = T]
               /\ pq' = pq \ {T}
               /\ slk' = 0
               /\ pcw' = [pcw EXCEPT ![i] = "run"]
               /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, editing, remote, acked, ans, sent, seen, have, rq, ld,
                              pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

\* No step starts: wait on the condvar if another agent runs one, else idle (the lock is dropped before the sleep).
NoStart(i) == /\ pcw[i] = "choose"
              /\ have[i] \cap Allowed = {}
              /\ slk' = 0
              /\ pcw' = [pcw EXCEPT ![i] = IF Running THEN "cv" ELSE "ia"]
              /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                             pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

\* End of the step: takes the slot mutex, may find the worker answered when no other step runs, notifies.
End(i) == /\ pcw[i] = "run" /\ slk = 0
          /\ run' = [run EXCEPT ![i] = NONE]
          /\ ans' \in IF \A j \in Sl : j = i \/ run[j] = NONE THEN BOOLEAN ELSE {ans}
          /\ pcw' = Notify(i, "top")
          /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, pq, sent, seen, have, rq, ld,
                         pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

IA(i) == /\ pcw[i] = "ia" /\ xp = 0
         /\ xp' = i
         /\ pcw' = [pcw EXCEPT ![i] = "s1"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

S1(i) == /\ pcw[i] = "s1"
         /\ IF EW # seen[i]
              THEN /\ xp' = 0 /\ pcw' = [pcw EXCEPT ![i] = "top"]
              ELSE /\ xp' = xp /\ pcw' = [pcw EXCEPT ![i] = "s2"]
         /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

S2(i) == /\ pcw[i] = "s2"
         /\ SW' = SW + 1
         /\ pcw' = [pcw EXCEPT ![i] = "dk"]
         /\ UNCHANGED <<EW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Dk(i) == /\ pcw[i] = "dk"
         /\ IF EW # seen[i]
              THEN /\ SW' = SW - 1 /\ xp' = 0 /\ pcw' = [pcw EXCEPT ![i] = "top"]
              ELSE /\ SW' = SW /\ xp' = xp /\ pcw' = [pcw EXCEPT ![i] = "kern"]
         /\ UNCHANGED <<EW, EL, SL, edit, held, res, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                        pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Kern(i) == /\ pcw[i] = "kern"
           /\ pcw' = [pcw EXCEPT ![i] = IF EW = seen[i] THEN "fw" ELSE "dec"]
           /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                          pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Woken(i) == /\ pcw[i] = "woken"
            /\ pcw' = [pcw EXCEPT ![i] = "dec"]
            /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                           pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

Dec(i) == /\ pcw[i] = "dec"
          /\ SW' = SW - 1
          /\ pcw' = [pcw EXCEPT ![i] = "s1"]
          /\ UNCHANGED <<EW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                         pcl, k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

AgentStep(i) == Top(i) \/ Snap(i) \/ Lock(i) \/ Body(i) \/ Chk1(i) \/ A0(i) \/ A1(i) \/ A2(i) \/ A3(i) \/ A4(i) \/ A5(i)
                \/ Chk2(i) \/ Q0(i) \/ Q1(i) \/ Q2(i) \/ Q3(i) \/ Q4(i) \/ Q5(i)
                \/ (\E T \in Kinds : Start(i, T)) \/ NoStart(i) \/ End(i)
                \/ IA(i) \/ S1(i) \/ S2(i) \/ Dk(i) \/ Kern(i) \/ Woken(i) \/ Dec(i)

\* ---- the leader ----

L0 == /\ pcl = "l0"
      /\ IF k > Len(Plan)
           THEN /\ pcl' = "done" /\ cur' = cur
           ELSE /\ pcl' = "e1" /\ cur' = Plan[k]
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, sl, ph, runit, fruns, nret, pcn, pcz>>

E1 == /\ pcl = "e1"
      /\ edit' = cur /\ pcl' = "e2"
      /\ UNCHANGED <<EW, SW, EL, SL, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

E2 == /\ pcl = "e2"
      /\ EW' = EW + 1 /\ pcl' = "e3"
      /\ UNCHANGED <<SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

E3 == /\ pcl = "e3"
      /\ ld' = [ld EXCEPT ![LEAD] = (SW # 0)] /\ pcl' = "e4"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

E4 == /\ pcl = "e4"
      /\ pcw' = WakeW(ld[LEAD]) /\ pcl' = "w1" /\ ph' = 1
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                     k, cur, sl, runit, fruns, nret, pcn, pcz>>

\* wait_all: read the epoch, test, leader_wait.
W1 == /\ pcl = "w1"
      /\ sl' = EL /\ pcl' = "w2"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, ph, runit, fruns, nret, pcn, pcz>>

W2 == /\ pcl = "w2"
      /\ pcl' = IF (ph = 1 /\ held = 1) \/ (ph = 2 /\ held = 0) \/ res THEN (IF ph = 1 THEN "x1" ELSE "nx") ELSE "w3"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

W3 == /\ pcl = "w3"
      /\ pcl' = IF EL # sl THEN "w1" ELSE "w4"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

W4 == /\ pcl = "w4"
      /\ SL' = SL + 1 /\ pcl' = "w5"
      /\ UNCHANGED <<EW, SW, EL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

W5 == /\ pcl = "w5"
      /\ pcl' = IF EL = sl THEN "w6" ELSE "w7"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

W6 == /\ pcl = "w6"
      /\ pcl' = IF EL = sl THEN "lfw" ELSE "w7"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

W7 == /\ pcl = "w7"
      /\ SL' = SL - 1 /\ pcl' = "w3"
      /\ UNCHANGED <<EW, SW, EL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

X1 == /\ pcl = "x1"
      /\ runit' = ~res /\ pcl' = "f1"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, fruns, nret, pcn, pcz>>

F1 == /\ pcl = "f1"
      /\ IF runit
           THEN /\ fruns' = [fruns EXCEPT ![k] = @ + 1] /\ pcl' = "f2"
           ELSE /\ fruns' = fruns /\ pcl' = "d1"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, nret, pcn, pcz>>

F2 == /\ pcl = "f2"
      /\ pcl' = "d1"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

D1 == /\ pcl = "d1"
      /\ edit' = {} /\ pcl' = "d2"
      /\ UNCHANGED <<EW, SW, EL, SL, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

D2 == /\ pcl = "d2"
      /\ EW' = EW + 1 /\ pcl' = "d3"
      /\ UNCHANGED <<SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

D3 == /\ pcl = "d3"
      /\ ld' = [ld EXCEPT ![LEAD] = (SW # 0)] /\ pcl' = "d4"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq,
                     k, cur, sl, ph, runit, fruns, nret, pcn, pcz>>

D4 == /\ pcl = "d4"
      /\ pcw' = WakeW(ld[LEAD])
      /\ IF WAITREL THEN pcl' = "w1" /\ ph' = 2 ELSE pcl' = "nx" /\ ph' = ph
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                     k, cur, sl, runit, fruns, nret, pcn, pcz>>

NX == /\ pcl = "nx"
      /\ k' = k + 1 /\ nret' = nret + 1 /\ pcl' = "l0"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     cur, sl, ph, runit, fruns, pcn, pcz>>

LeaderStep == L0 \/ E1 \/ E2 \/ E3 \/ E4 \/ W1 \/ W2 \/ W3 \/ W4 \/ W5 \/ W6 \/ W7 \/ X1 \/ F1 \/ F2 \/ D1 \/ D2 \/ D3 \/ D4 \/ NX

\* ---- the kernel's wake of the leader's sleep: part of the bumpers' steps (WakeL); the leader's kernel compare is W6 (w6 -> lfw or w7) ----

\* ---- the environment: frames of the worker (their sender bumps the worker's epoch, as `Transport::send` does) ----

N0 == /\ pcn = "n0" /\ sent < F
      /\ \E T \in Kinds : pq' = pq \cup {T}
      /\ sent' = sent + 1 /\ pcn' = "n1"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pcw, seen, have, rq, ld,
                     pcl, k, cur, sl, ph, runit, fruns, nret, pcz>>

N1 == /\ pcn = "n1"
      /\ EW' = EW + 1 /\ pcn' = "n2"
      /\ UNCHANGED <<SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     pcl, k, cur, sl, ph, runit, fruns, nret, pcz>>

N2 == /\ pcn = "n2"
      /\ ld' = [ld EXCEPT ![ENV] = (SW # 0)] /\ pcn' = "n3"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq,
                     pcl, k, cur, sl, ph, runit, fruns, nret, pcz>>

N3 == /\ pcn = "n3"
      /\ pcw' = WakeW(ld[ENV]) /\ pcn' = "n0"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, seen, have, rq, ld,
                     pcl, k, cur, sl, ph, runit, fruns, nret, pcz>>

EnvBump == N1 \/ N2 \/ N3
EnvStep == N0 \/ EnvBump

\* ---- finish: after every agent returned, the outcome word, then the bump of the leader (roles/results.rs 35-43) ----

Z0 == /\ pcz = "z0" /\ \A i \in Sl : pcw[i] = "exit"
      /\ pcz' = "z1"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     pcl, k, cur, sl, ph, runit, fruns, nret, pcn>>

Z1 == /\ pcz = "z1"
      /\ res' = TRUE /\ pcz' = "z2"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     pcl, k, cur, sl, ph, runit, fruns, nret, pcn>>

Z2 == /\ pcz = "z2"
      /\ EL' = EL + 1 /\ pcz' = "z3"
      /\ UNCHANGED <<EW, SW, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     pcl, k, cur, sl, ph, runit, fruns, nret, pcn>>

Z3 == /\ pcz = "z3"
      /\ ld' = [ld EXCEPT ![FIN] = (SL # 0)] /\ pcz' = "z4"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq,
                     pcl, k, cur, sl, ph, runit, fruns, nret, pcn>>

Z4 == /\ pcz = "z4"
      /\ pcl' = WakeL(ld[FIN]) /\ pcz' = "zd"
      /\ UNCHANGED <<EW, SW, EL, SL, edit, held, res, xp, slk, editing, remote, acked, run, ans, pq, sent, pcw, seen, have, rq, ld,
                     k, cur, sl, ph, runit, fruns, nret, pcn>>

FinStep == Z0 \/ Z1 \/ Z2 \/ Z3 \/ Z4

Next == (\E i \in Sl : AgentStep(i)) \/ LeaderStep \/ EnvStep \/ FinStep

\* Weak fairness of each process's steps. The frame sender's first step (N0) is not fair: frames may never come.
Spec == Init /\ [][Next]_vars
        /\ (\A i \in Sl : WF_vars(AgentStep(i))) /\ WF_vars(LeaderStep) /\ WF_vars(EnvBump) /\ WF_vars(FinStep)

TypeOK ==
  /\ EW \in Nat /\ EL \in Nat /\ SW \in 0..N /\ SL \in 0..1 /\ held \in 0..1 /\ res \in BOOLEAN
  /\ edit \subseteq Fields /\ editing \subseteq Fields /\ remote \subseteq Fields /\ cur \subseteq Fields
  /\ xp \in 0..N /\ slk \in 0..N /\ pq \subseteq Kinds /\ sent \in 0..F
  /\ run \in [Sl -> Kinds \cup {NONE}]

\* ---- safety ----

\* (a) f never runs while a step touching its fields runs (CONTRACT S1, S4).
Safety == pcl = "f2" => \A i \in Sl : run[i] = NONE \/ run[i] \cap cur = {}

\* (b) While f runs, the worker holds exactly the fields of this edit and has told the leader (HELD = 1).
\* (`acked` is the slot's own note, set after the call that stores HELD returns, so it may lag.)
Held == pcl = "f2" => (remote = cur /\ held = 1)

\* (c) f runs at most once per edit; and when it did not run, the worker had a result.
Once == /\ \A m \in DOMAIN fruns : fruns[m] <= 1
        /\ pcl = "nx" => (fruns[k] = 1 \/ res)

\* (d) No step starts between the hold and the release: a start never touches a held field.
NoStartInHold ==
  [][\A i \in Sl : (run[i] = NONE /\ run'[i] # NONE) => (run'[i] \cap editing = {} /\ run'[i] \cap remote = {})]_vars

\* (e) No lost wake-up. A sleeper in the kernel whose word has moved on is being woken: a bumper has
\* incremented and has not yet loaded the count, or has loaded a non-zero count and not yet woken.
InFlightW == \/ pcl \in {"e3", "d3"} \/ (pcl \in {"e4", "d4"} /\ ld[LEAD])
             \/ pcn = "n2" \/ (pcn = "n3" /\ ld[ENV])
InFlightL == \/ \E i \in Sl : (pcw[i] \in {"a3", "q3"} \/ (pcw[i] \in {"a4", "q4"} /\ ld[i]))
             \/ pcz = "z3" \/ (pcz = "z4" /\ ld[FIN])
NoLostWakeup ==
  /\ \A i \in Sl : (pcw[i] = "fw" /\ EW # seen[i]) => InFlightW
  /\ (pcl = "lfw" /\ EL # sl) => InFlightL

\* The count of each word is exact.
CountExact ==
  /\ SW = Cardinality({i \in Sl : pcw[i] \in {"dk", "kern", "fw", "woken", "dec"}})
  /\ SL = IF pcl \in {"w5", "w6", "lfw", "w7"} THEN 1 ELSE 0

\* ---- liveness, under weak fairness ----

\* (f) Every edit returns.
EditsReturn == \A m \in 1..Len(Plan) : <>(nret >= m)

\* (g) Every hold is released, unless the worker has a result (no agent is left to release it).
HoldReleased == (remote # {}) ~> (remote = {} \/ res)

\* A request is taken and a withdrawal is served.
Pending == (edit # {} /\ remote = {}) \/ (edit = {} /\ remote # {} /\ acked)
RequestServed == (edit # {} /\ remote = {}) ~> (remote # {} \/ res)
WithdrawServed == (edit = {} /\ remote # {} /\ acked) ~> (remote = {} \/ res)

\* (h) No agent sleeps for ever while a request or a withdrawal is pending.
NoForeverSleep == \A i \in Sl : (pcw[i] = "fw" /\ Pending /\ ~res) ~> (pcw[i] # "fw" \/ ~Pending \/ res)

====
