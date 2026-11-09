---- MODULE Lane ----
\* One lossless route between hosts, capacity C, N frames: src/backends/mpi/cross.rs, Out and In.
\* Sender: p frames sent, c the largest credit received (Out::poll). It sends frame p only when
\* p - c < C (Out::send), and its buffer is free again once frame p - C's send completed (Out::put).
\* Receiver: q frames taken (In::c); a receive is posted for each frame q .. q + C - 1 (In::post);
\* `got` frames have matched their receive. Taking frame q posts the receive of q + C and makes a
\* credit due; a credit is sent only when the last one has completed (In::credit), carrying q then.
\* MPI keeps one sender's messages of one tag in order: frames and credits are FIFO sequences, and a
\* frame matches the receive of its own index because the receives are posted in index order.
\* Interleaving semantics. A credit send completes when the sender's posted receive takes it (the
\* slowest completion MPI allows); a frame's send completes when its receive matches.
\*
\* Not modelled: lossy routes and the pending frame; payloads (frames are indices); the agents and
\* the transport lock (each action is one call under XPORT); the end (quiesce); several lanes.
EXTENDS Naturals, Sequences

\* EXTRA is 0 for the code; 1 is the mutant whose sender takes one frame more room than its credits
\* give, which MCLaneOver must find.
CONSTANTS C, N, EXTRA

VARIABLES
  p, c,           \* sender: frames sent, largest credit received
  wire,           \* frames in flight, by index
  got, q,         \* receiver: frames matched, frames taken
  due, out,       \* receiver: a credit due; a credit send not yet completed
  cwire           \* credits in flight

vars == <<p, c, wire, got, q, due, out, cwire>>

Init == p = 0 /\ c = 0 /\ wire = <<>> /\ got = 0 /\ q = 0 /\ due = FALSE /\ out = FALSE /\ cwire = <<>>

\* Out::send with room: the frame goes on the wire.
Send == /\ p < N /\ p - c < C + EXTRA
        /\ wire' = Append(wire, p) /\ p' = p + 1
        /\ UNCHANGED <<c, got, q, due, out, cwire>>

\* MPI matches the head frame with its posted receive (In::head's MPI_Test sees it).
Match == /\ wire # <<>>
         /\ got' = got + 1 /\ wire' = Tail(wire)
         /\ UNCHANGED <<p, c, q, due, out, cwire>>

\* In::credit: send the due credit if the last one completed.
Credit == /\ due /\ ~out
          /\ cwire' = Append(cwire, q) /\ out' = TRUE /\ due' = FALSE
          /\ UNCHANGED <<p, c, wire, got, q>>

\* In::take: frame q has matched; take it, post q + C, owe a credit, and try to send it at once.
Take == /\ q < got
        /\ q' = q + 1
        /\ IF ~out THEN cwire' = Append(cwire, q + 1) /\ out' = TRUE /\ due' = FALSE
                   ELSE due' = TRUE /\ UNCHANGED <<cwire, out>>
        /\ UNCHANGED <<p, c, wire, got>>

\* Out::poll: the sender's posted credit receive takes the head credit; that completes its send.
Poll == /\ cwire # <<>>
        /\ c' = IF Head(cwire) > c THEN Head(cwire) ELSE c
        /\ cwire' = Tail(cwire) /\ out' = FALSE
        /\ UNCHANGED <<p, wire, got, q, due>>

Next == Send \/ Match \/ Credit \/ Take \/ Poll

Spec == Init /\ [][Next]_vars /\ WF_vars(Send) /\ WF_vars(Match) /\ WF_vars(Credit) /\ WF_vars(Take) /\ WF_vars(Poll)

\* The chain: c <= q <= got <= p <= c + C.
Chain == c <= q /\ q <= got /\ got <= p /\ p <= c + C

\* Every frame in flight has its receive posted: its index is below q + C (cross.rs, Receiver).
Posted == \A k \in 1..Len(wire) : wire[k] < q + C

\* Frames on the wire are the next ones in order (T1).
InOrder == \A k \in 1..Len(wire) : wire[k] = got + k - 1

\* Out::put waits only when it can send, p - c < C, and then frame p - C was credited: it matched.
NoBlock == (p < N /\ p - c < C /\ p >= C) => got > p - C

\* At most one credit send outstanding (In::credit).
OneCredit == Len(cwire) <= 1 /\ (out <=> Len(cwire) = 1)

\* Liveness: every frame is taken, and its credit reaches the sender.
AllCredited == <>(q = N /\ c = N)

====
