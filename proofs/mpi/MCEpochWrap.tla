---- MODULE MCEpochWrap ----
\* The width claim: at W = 2 (a 1-bit epoch word) and K = 2 bumps, a waiter can miss a wake-up.
\* Expected result: TLC reports a violation of NoLostWakeup. The trace is counterexamples/epoch-wrap.txt.
EXTENDS Epoch
====
