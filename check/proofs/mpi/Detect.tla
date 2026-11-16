---- MODULE Detect ----
\* The leaders' deadlock detector: backends/mpi/roles/detect.rs. One worker per host, H hosts. A worker is
\* active (a step may run), quiet (no step runs, its agents idle with no deadline: RUN = 0 and
\* IDLE = EPOCH + 1) or done (it has a result). An active worker may send a message to another host
\* (a frame or a credit, transport/cross/), go quiet, or end. A message in flight arrives at its receiver: the
\* receiver's `got` count and epoch grow, and the worker is active again (poll_lanes bumps the epoch;
\* the agent re-checks). The leader of a host may act on its worker between its rounds (a send, an
\* edit, a stop): the worker's epoch grows and it is active.
\*
\* Rounds. A leader reads its worker's four words one at a time, while every worker moves: quiet,
\* epoch, sent, got. Then it posts its part of round k: unstable unless the snapshot is quiet and
\* equal to the one it gave round k - 1; the sent and got counts. Round k completes for a leader once
\* every leader posted it; the leader then reads the sums. Stuck when no leader is unstable and the
\* sums of sent and got are equal. STABLE = FALSE is the mutant without the comparison with the last
\* round (one collection): MCDetectOnce must find a verdict on a run that is not stuck.
\*
\* Property. Sound: a Stuck verdict is given only when every worker is quiet or done and no message
\* is in flight, which no action can leave (Terminated is stable: only an arrival or a leader act wakes
\* a quiet worker, and a leader that took a verdict acts no more in this model).
\*
\* Not modelled: several workers or lanes per host (their words are more fields of the same read);
\* the words' widths (EPOCH wraps after 2^32 bumps, epoch.rs); a leader in its own code (it posts no
\* round, so no verdict is reached: shown by the model only as rounds that stop); peer messages (more
\* sent and got counts); quiesce and the end of the rounds (Over).
EXTENDS Naturals, FiniteSets

CONSTANTS H, BUDGET, ROUNDS, STABLE

Hosts == 0..H-1
Fields == 0..3   \* quiet, epoch, sent, got

VARIABLES
  st,        \* per worker: "active", "quiet" or "done"
  epoch,     \* per worker
  sent, got, \* per worker: messages sent, messages arrived
  wire,      \* per worker: messages in flight to it
  budget,    \* messages and leader acts left to the whole run
  rd,        \* per leader: the round it is in
  pos,       \* per leader: the next field to read, 4 when read and posted
  snap,      \* per leader: the snapshot being read
  last,      \* per leader: the snapshot given to the previous round, or <<>> before the first
  part,      \* part[l][k]: leader l's part of round k, <<unstable, sent, got>>, or <<>>
  verdict    \* TRUE once some leader read a Stuck round

vars == <<st, epoch, sent, got, wire, budget, rd, pos, snap, last, part, verdict>>

Init == /\ st = [w \in Hosts |-> "active"] /\ epoch = [w \in Hosts |-> 0]
        /\ sent = [w \in Hosts |-> 0] /\ got = [w \in Hosts |-> 0] /\ wire = [w \in Hosts |-> 0]
        /\ budget = BUDGET
        /\ rd = [l \in Hosts |-> 0] /\ pos = [l \in Hosts |-> 0]
        /\ snap = [l \in Hosts |-> [f \in Fields |-> 0]] /\ last = [l \in Hosts |-> <<>>]
        /\ part = [l \in Hosts |-> [k \in 0..ROUNDS |-> <<>>]]
        /\ verdict = FALSE

Workers == <<st, epoch, sent, got, wire, budget>>
Leaders == <<rd, pos, snap, last, part, verdict>>

\* An active worker sends a message to another host.
Send(w, v) == /\ v # w /\ st[w] = "active" /\ budget > 0
              /\ sent' = [sent EXCEPT ![w] = @ + 1] /\ wire' = [wire EXCEPT ![v] = @ + 1]
              /\ budget' = budget - 1
              /\ UNCHANGED <<st, epoch, got>> /\ UNCHANGED Leaders

Quiet(w) == /\ st[w] = "active" /\ st' = [st EXCEPT ![w] = "quiet"]
            /\ UNCHANGED <<epoch, sent, got, wire, budget>> /\ UNCHANGED Leaders

Finish(w) == /\ st[w] = "active" /\ st' = [st EXCEPT ![w] = "done"]
             /\ UNCHANGED <<epoch, sent, got, wire, budget>> /\ UNCHANGED Leaders

\* A message arrives: the receiver observes it, its epoch moves, and it re-checks its conditions. A
\* done worker keeps observing (quiesce) but never runs again.
Arrive(w) == /\ wire[w] > 0
             /\ wire' = [wire EXCEPT ![w] = @ - 1] /\ got' = [got EXCEPT ![w] = @ + 1]
             /\ epoch' = [epoch EXCEPT ![w] = @ + 1]
             /\ st' = [st EXCEPT ![w] = IF @ = "done" THEN "done" ELSE "active"]
             /\ UNCHANGED <<sent, budget>> /\ UNCHANGED Leaders

\* The leader acts on its worker between rounds (pos = 0: not reading).
Act(l) == /\ pos[l] = 0 /\ st[l] # "done" /\ budget > 0 /\ ~verdict
          /\ epoch' = [epoch EXCEPT ![l] = @ + 1] /\ st' = [st EXCEPT ![l] = "active"]
          /\ budget' = budget - 1
          /\ UNCHANGED <<sent, got, wire>> /\ UNCHANGED Leaders

Value(w, f) == CASE f = 0 -> IF st[w] = "active" THEN 0 ELSE 1
               [] f = 1 -> epoch[w]
               [] f = 2 -> sent[w]
               [] f = 3 -> got[w]

\* Read one word of the snapshot.
Read(l) == /\ pos[l] < 4 /\ rd[l] <= ROUNDS
           /\ snap' = [snap EXCEPT ![l][pos[l]] = Value(l, pos[l])]
           /\ pos' = [pos EXCEPT ![l] = @ + 1]
           /\ IF pos[l] = 3 THEN
                LET s == [snap[l] EXCEPT ![3] = Value(l, 3)]
                    stable == s[0] = 1 /\ (~STABLE \/ last[l] = s)
                IN /\ part' = [part EXCEPT ![l][rd[l]] = <<IF stable THEN 0 ELSE 1, s[2], s[3]>>]
                   /\ last' = [last EXCEPT ![l] = s]
              ELSE UNCHANGED <<part, last>>
           /\ UNCHANGED <<rd, verdict>> /\ UNCHANGED Workers

RECURSIVE Sum(_, _, _)
Sum(k, i, S) == IF S = {} THEN 0 ELSE LET m == CHOOSE m \in S : TRUE IN part[m][k][i] + Sum(k, i, S \ {m})

\* Round rd[l] has completed: every leader posted it. The leader reads the sums and starts the next.
Complete(l) == /\ pos[l] = 4 /\ \A m \in Hosts : part[m][rd[l]] # <<>>
               /\ LET k == rd[l] IN
                  verdict' = (verdict \/ (Sum(k, 1, Hosts) = 0 /\ Sum(k, 2, Hosts) = Sum(k, 3, Hosts)))
               /\ rd' = [rd EXCEPT ![l] = @ + 1] /\ pos' = [pos EXCEPT ![l] = 0]
               /\ UNCHANGED <<snap, last, part>> /\ UNCHANGED Workers

Next == \/ \E w, v \in Hosts : Send(w, v)
        \/ \E w \in Hosts : Quiet(w) \/ Finish(w) \/ Arrive(w) \/ Act(w) \/ Read(w) \/ Complete(w)

Spec == Init /\ [][Next]_vars

Terminated == \A w \in Hosts : st[w] # "active" /\ wire[w] = 0

\* A verdict is given only on a terminated run.
Sound == verdict => Terminated

\* Bound the state space: rounds stop after ROUNDS.
Bounded == \A l \in Hosts : rd[l] <= ROUNDS + 1
====
