// This backend has no lane route at all, so it cannot promise a reliable one. Declaring
// `LANE_UNAVAILABLE` is the refusal, and this fixture is what proves the refusal is reachable.
//~ says: requires `reliable_lanes`
use trame::require;

require!(reliable_lanes);
