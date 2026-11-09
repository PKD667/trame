# The latency bounds of CONTRACT.md, Latency: clauses Q1 (median) and Q2 (tail, the 99th percentile). A bound is a
# round trip in nanoseconds, per reference deployment, placement and message size, and does not depend on the backend:
# a backend is accepted if its round trips on the deployment are within the same bounds.
#
#   q.<deployment>.<placement>.<size> = { p50 = ns | null; p99 = ns | null; }
#
# A deployment is named for the hardware and network the bounds are set on:
#   grvingt: Nancy (Grid'5000), grvingt nodes, Omni-Path, Open MPI ob1 over tcp on ib0.
# placement: "same-host" (both workers on one trame host) or "cross-host" (the two on hosts of different machines).
# A bound that is null is not set: the round trips are measured and no clause is claimed for it (the report says
# "no bound"). No number has been measured on a reference deployment yet, so none is set here. To set one, measure
# (nix run .#suite, nix build .#suite-report), read the quantile off the report, and write it here with the date.
{ lib }:
let
  sizes = [ 16 1024 65536 ]; # a subset of lab/bench/common.rs SIZES
  placements = [ "same-host" "cross-host" ];
  unset = { p50 = null; p99 = null; };
  deployment = lib.genAttrs placements (_: lib.genAttrs (map toString sizes) (_: unset));
in
{
  inherit sizes placements;
  q.grvingt = deployment;
  # The deployment whose bounds the latency experiment tags its rows with.
  reference = "grvingt";
}
