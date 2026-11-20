---- MODULE MCDetectOnce ----
\* Detect.tla without the comparison with the last round (STABLE = FALSE): one collection of words
\* read one at a time can see every worker quiet and sent = got while a message is still in flight
\* or a worker active. Sound must be violated.
EXTENDS Detect
====
