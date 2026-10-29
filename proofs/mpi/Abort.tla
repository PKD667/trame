---- MODULE Abort ----
\* Abort and end of a run over several nodes. Code: backends/mpi/NODES.md "Abort" and "End";
\* backends/mpi/leader.rs leader_panicked (lines 28-37) and fail_if_aborted / end_run (results.rs 112-150);
\* results.rs record_abort (54-70): the abort bit is set on the node, once, and every local waiter is woken.
\* Each node has one relay (out sends, in receives) and one worker; the leader is rank 0 on the root
\* node Root, whose relay sends END to every node, itself included.
\* EndAlways = TRUE is the code: leader_panicked sets END even when an abort is already recorded.
\* EndAlways = FALSE is the code before that repair: it returned when the abort bit was set, so END was
\* not sent when the leader panicked after an abort arrived and before its next leader call. That
\* variant must violate Live (counterexamples/abort-leader-panic.txt), which shows the check can fail.
\* Not modelled: MPI's own progress (messages arrive in finite time, WF), the process exit and
\* MPI_Finalize, timeouts, the worker's result and exit records, the publication objects.
\* Panics are environment steps (not fair); the liveness claim is conditioned on an abort having happened.
EXTENDS Naturals

CONSTANTS Nodes, Root, EndAlways

VARIABLES
  ab,        \* nodes whose abort bit is set
  pend,      \* nodes whose out has an abort record to send (a local record, not yet sent)
  flight,    \* nodes with an abort record in flight to them
  endw,      \* the root's END word (end_run)
  endSent,   \* the root's out has sent END to every node
  endFlight, \* nodes with END in flight to them
  stopped,   \* nodes whose relay has stopped
  wdone,     \* workers that have ended (panicked or woken by the abort)
  ldone      \* the leader has ended

vars == <<ab, pend, flight, endw, endSent, endFlight, stopped, wdone, ldone>>

Init == /\ ab = {} /\ pend = {} /\ flight = {} /\ endw = FALSE /\ endSent = FALSE
        /\ endFlight = {} /\ stopped = {} /\ wdone = {} /\ ldone = FALSE

\* A worker panics: record_abort on its node (only the first record is kept and sent), then it ends.
WPanic(n) == /\ n \notin wdone
             /\ IF n \in ab
                  THEN /\ UNCHANGED <<ab, pend>>
                  ELSE /\ ab' = ab \cup {n} /\ pend' = pend \cup {n}
             /\ wdone' = wdone \cup {n}
             /\ UNCHANGED <<flight, endw, endSent, endFlight, stopped, ldone>>

\* The leader panics (leader.rs leader_panicked).
LPanic == /\ ~ldone
          /\ IF Root \in ab
               THEN /\ endw' = (IF EndAlways THEN TRUE ELSE endw)
                    /\ UNCHANGED <<ab, pend>>
               ELSE /\ ab' = ab \cup {Root} /\ pend' = pend \cup {Root} /\ endw' = TRUE
          /\ ldone' = TRUE
          /\ UNCHANGED <<flight, endSent, endFlight, stopped, wdone>>

\* A rank woken by the abort bit ends (fail_if_aborted for the leader, the wait for a worker).
WWake(n) == /\ n \in ab /\ n \notin wdone
            /\ wdone' = wdone \cup {n}
            /\ UNCHANGED <<ab, pend, flight, endw, endSent, endFlight, stopped, ldone>>

LWake == /\ Root \in ab /\ ~ldone
         /\ ldone' = TRUE /\ endw' = TRUE            \* fail_if_aborted: end_run, then panic
         /\ UNCHANGED <<ab, pend, flight, endSent, endFlight, stopped, wdone>>

\* The out thread of node n sends its abort record once, to every other node.
Out(n) == /\ n \in pend
          /\ pend' = pend \ {n}
          /\ flight' = flight \cup (Nodes \ {n})
          /\ UNCHANGED <<ab, endw, endSent, endFlight, stopped, wdone, ldone>>

\* The in thread of node m records an arriving abort: sets the bit and wakes the local waiters.
In(m) == /\ m \in flight
         /\ ab' = ab \cup {m}
         /\ flight' = flight \ {m}
         /\ UNCHANGED <<pend, endw, endSent, endFlight, stopped, wdone, ldone>>

\* The root's out sends END to every node once, when the END word is set.
RootOut == /\ endw /\ ~endSent
           /\ endSent' = TRUE
           /\ endFlight' = Nodes
           /\ UNCHANGED <<ab, pend, flight, endw, stopped, wdone, ldone>>

\* The relay of node m stops when END arrives.
EndIn(m) == /\ m \in endFlight
            /\ endFlight' = endFlight \ {m}
            /\ stopped' = stopped \cup {m}
            /\ UNCHANGED <<ab, pend, flight, endw, endSent, wdone, ldone>>

Env == \E n \in Nodes : WPanic(n)
Progress == LWake \/ \E n \in Nodes : WWake(n) \/ Out(n) \/ In(n) \/ EndIn(n)

Next == LPanic \/ Env \/ RootOut \/ Progress

\* Weak fairness of every rank's and every node's progress step. Panics are not fair.
Spec == Init /\ [][Next]_vars
          /\ WF_vars(LWake) /\ WF_vars(RootOut)
          /\ \A n \in Nodes : WF_vars(WWake(n)) /\ WF_vars(Out(n)) /\ WF_vars(In(n)) /\ WF_vars(EndIn(n))

AllDone == ldone /\ wdone = Nodes /\ stopped = Nodes

\* Liveness. An abort on some node: every rank ends and every relay stops.
Live == (ab # {}) ~> AllDone

\* Liveness. An abort on some node reaches every node's abort bit.
Reach == (ab # {}) ~> (ab = Nodes)

\* Safety: END is set only after an abort has been recorded.
EndAfterAbort == endw => ab # {}

====
