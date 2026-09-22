// One process: an atomic orders this participant's own threads and nothing else, so there is no
// second participant for a sharing domain to mean.
//~ says: requires `atomic_scope(domain)`
use trame::require;

require!(atomic_scope(domain));
