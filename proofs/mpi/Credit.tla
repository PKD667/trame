---- MODULE Credit ----
\* One lossless route from a sender on node A to a receiver on node B, capacity C, N frames.
\* Code: backends/mpi/relay_out.rs (the out thread's scan: Out::run, scan, flush; `next` = F, `credited`),
\* backends/mpi/relay_routes.rs (frames: F..P_A read at the scan's start; credit: C_B read later in the
\* same scan and sent when it moved), NODES.md "A route between nodes" steps 1-5 (the words P, C, held).
\* MPI keeps one pair's messages in order: each direction is a FIFO sequence.
\* Interleaving semantics, every access SeqCst (as epoch.rs states).
\*
\* Not modelled: the scan's flush delay (records are enqueued when pushed, which only moves arrival
\* later and changes no order); the second route of a node, which is what puts frames and credits of
\* different routes in one scan (credits here travel B to A, so one route alone has no shared scan);
\* ring slot contents (slots are abstracted to indices); lossy routes; the wake protocol (the out thread
\* here may scan at any time, a superset of the wakes it can get: safety is checked over more schedules).
EXTENDS Naturals, Sequences

CONSTANTS C, N

VARIABLES
  PA, CA,         \* A's words: P_A (frames written by the sender), C_A (last credit stored by A's in)
  FA, pcA, snap,  \* A's out: F (next frame to send), its scan phase, and the snapshot of P_A it read
  wireAB,         \* frames in flight A to B (indices), FIFO
  PB, CB,         \* B's words: P_B (frames stored by B's in), C_B (frames consumed by v)
  credited,       \* B's out: the last credit sent
  wireBA          \* credits in flight B to A (values of C), FIFO

vars == <<PA, CA, FA, pcA, snap, wireAB, PB, CB, credited, wireBA>>

Init == /\ PA = 0 /\ CA = 0 /\ FA = 0 /\ pcA = "idle" /\ snap = 0
        /\ wireAB = <<>> /\ PB = 0 /\ CB = 0 /\ credited = 0 /\ wireBA = <<>>

\* The sender writes frame PA if the route is not Full: held = PA - CA < C (NODES.md, Full).
Send == /\ PA < N /\ PA - CA < C
        /\ PA' = PA + 1
        /\ UNCHANGED <<CA, FA, pcA, snap, wireAB, PB, CB, credited, wireBA>>

\* A's out scan: read P_A once (relay_routes.rs frames, the word load), then send F..snap-1.
AStart == /\ pcA = "idle"
          /\ snap' = PA
          /\ pcA' = "send"
          /\ UNCHANGED <<PA, CA, FA, wireAB, PB, CB, credited, wireBA>>

ASend == /\ pcA = "send"
         /\ IF FA < snap
              THEN /\ wireAB' = Append(wireAB, FA) /\ FA' = FA + 1 /\ pcA' = "send"
              ELSE /\ pcA' = "idle" /\ UNCHANGED <<wireAB, FA>>
         /\ UNCHANGED <<PA, CA, snap, PB, CB, credited, wireBA>>

\* A's in stores a credit as C_A (relay_in: stores the value; credits arrive in FIFO order).
AIn == /\ wireBA # <<>>
       /\ CA' = Head(wireBA)
       /\ wireBA' = Tail(wireBA)
       /\ UNCHANGED <<PA, FA, pcA, snap, wireAB, PB, CB, credited>>

\* B's in writes the arriving frame into its slot and stores P_B. The frame's index must be P_B.
BIn == /\ wireAB # <<>>
       /\ Head(wireAB) = PB
       /\ PB' = PB + 1
       /\ wireAB' = Tail(wireAB)
       /\ UNCHANGED <<PA, CA, FA, pcA, snap, CB, credited, wireBA>>

\* The receiver v takes a stored frame: C_B moves. This is the "Took" of the spec.
BTake == /\ CB < PB
         /\ CB' = CB + 1
         /\ UNCHANGED <<PA, CA, FA, pcA, snap, wireAB, PB, credited, wireBA>>

\* B's out reads C_B and sends it as a credit if it moved since the last credit (relay_out.rs credit).
BOut == /\ CB > credited
        /\ wireBA' = Append(wireBA, CB)
        /\ credited' = CB
        /\ UNCHANGED <<PA, CA, FA, pcA, snap, wireAB, PB, CB>>

Next == Send \/ AStart \/ ASend \/ AIn \/ BIn \/ BTake \/ BOut

Spec == Init /\ [][Next]_vars
          /\ WF_vars(Send) /\ WF_vars(AStart) /\ WF_vars(ASend) /\ WF_vars(AIn)
          /\ WF_vars(BIn) /\ WF_vars(BTake) /\ WF_vars(BOut)

\* Safety. The chain of NODES.md: C_A <= C_B <= P_B <= F <= P_A <= C_A + C.
Chain == /\ CA <= CB /\ CB <= PB /\ PB <= FA /\ FA <= PA /\ PA <= CA + C

\* Occupancy: B's ring holds at most C frames at any time.
Occupancy == PB - CB <= C

\* No over-send: every frame written is within the room B's receiver has freed.
NoOverSend == PA <= CB + C

\* The sender's view of room (C - (PA - CA)) never exceeds the true room (C - (PA - CB)).
ViewNoExceed == CA <= CB

\* Frames on the wire are the next ones in order, starting at P_B.
InOrder == \A k \in 1..Len(wireAB) : wireAB[k] = PB + k - 1

\* Liveness: every frame is eventually credited back to A.
AllCredited == <>(CA = N)

\* Expected to FAIL (the allowed Full of spec 5.2, not a bug). A "stale Full": every frame has arrived
\* at B (PA = PB), B's ring has room (PB - CB < C), and A still says Full (PA - CA = C). The room is a
\* consumption (Took, CB) whose credit has not reached A yet. The shortest run with this state is in
\* counterexamples/credit-stale-full.txt.
StaleFull == ~((PA - CA = C) /\ PA = PB /\ PB - CB < C)

====
