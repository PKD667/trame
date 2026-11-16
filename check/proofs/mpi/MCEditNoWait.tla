---- MODULE MCEditNoWait ----
\* Edit.tla: the leader does not wait for the release (broken variant), one agent, two edits of different fields. The constants are in MCEditNoWait.cfg.
EXTENDS Edit
MCPlan == <<{1}, {2}>>
====
