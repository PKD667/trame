# The protocol-mpi check (proofs/README.md): TLC runs on the models of proofs/mpi, each with its expected result.
# Each model is an abstraction; the verdict says what is explored, at which bounds, and what is not modelled.
{ pkgs, v }:
v.check {
  name = "protocol-mpi";
  property = "the wait protocol of a worker or the leader loses no wake-up at a 32-bit epoch word modelled at 16 with 3 bumps (Epoch.tla); a route keeps occupancy within its capacity and never over-sends (Credit.tla); an abort reaches every node, every rank ends and every relay stops, the leader included (Abort.tla). The wrap (EpochWrap), the stale Full (CreditStale) and the early return of the leader before its repair (AbortEarly) must have their counterexamples";
  model = "TLC 2.19 on proofs/mpi: one waiter and one bumper with SeqCst steps; one lossless route of capacity 2, 4 frames, two nodes; three nodes with one worker each and the leader on the root. Not the memory model below SeqCst, not timeouts, not the kernel beyond compare-and-sleep, not MPI's progress beyond weak fairness";
  subject = ../proofs/mpi;
  checker = pkgs.tlaplus;
  runtime = [ pkgs.bash pkgs.coreutils pkgs.gnugrep pkgs.gnused ];
  scope = { models = 6; epoch = "W=16 K=3 holds; W=2 K=2 counterexample"; credit = "C=2 N=4 holds; stale Full counterexample"; abort = "3 nodes holds; the early return before the repair violates Live"; tool = "tlaplus 1.7.4, TLC2 Version 2.19"; };
  command = ''
    # run.sh writes outcome.json itself; a model that fails is a verdict, not a build error.
    TLC=${pkgs.tlaplus}/bin/tlc sh ${../proofs/mpi}/run.sh $out || true
  '';
}
