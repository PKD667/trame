# The protocol-mpi check (proofs/README.md): TLC runs on the models of src/backends/mpi/proofs, each with its expected result.
# Each model is an abstraction; the verdict says what is explored, at which bounds, and what is not modelled.
# Called by lab/validate.nix through the descriptor (backend.nix, validate) with the flake's values.
{ pkgs, v, ... }:
{
  protocol-mpi = v.check {
    name = "protocol-mpi";
    property = "the wait protocol of a worker or the leader loses no wake-up at a 32-bit epoch word modelled at 16 with 3 bumps, and a wrapped word loses one (Epoch.tla, MCEpoch*.cfg); a lane keeps order and its credits, and an over-sending lane breaks the chain (Lane.tla, MCLane*.cfg); the leader's stuck detector is sound at two hosts, and a detector that skips its comparison is not (Detect.tla, MCDetect*.cfg); an edit that does not wait for its release loses the hold (Edit.tla, MCEditNoWait)";
    model = "TLC 2.19 on src/backends/mpi/proofs: one waiter and one bumper with SeqCst steps; one lossless lane of capacity 2 and 3, 5 and 7 frames, two and three nodes; three nodes with one worker each and the leader on the root; the edit handshake with its no-wait mutant. Not the memory model below SeqCst, not timeouts, not the kernel beyond compare-and-sleep, not MPI's progress beyond weak fairness; the four larger edit configurations did not finish (src/backends/mpi/proofs/README.md)";
    subject = ./proofs;
    checker = pkgs.tlaplus;
    runtime = [ pkgs.bash pkgs.coreutils pkgs.gnugrep pkgs.gnused ];
    scope = { models = 14; epoch = "count N=1,2,3 and with XPORT hold (W=16 K=3); flag N=2 counterexample, holds under XPORT; W=2 K=2 counterexample"; lane = "C=2 N=5 and C=3 N=7 hold; EXTRA=1 violates Chain"; detect = "2 hosts holds at 7,351,279 distinct states; the mutant that skips the comparison violates Sound"; edit = "only MCEditNoWait is adjudicated (violates Held); MCEdit, MCEdit2, MCEdit22, MCEditXportAck did not finish"; tool = "tlaplus 1.7.4, TLC2 Version 2.19"; };
    command = ''
      # run.sh writes outcome.json itself; a model that fails is a verdict, not a build error.
      TLC=${pkgs.tlaplus}/bin/tlc sh ${./proofs}/run.sh $out || true
    '';
  };
}
