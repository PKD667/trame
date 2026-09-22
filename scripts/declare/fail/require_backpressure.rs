// This backend refuses a send for being closed, never for lack of capacity, and has no peer
// whose pressure could be observed. A caller that needs flow control asks for it.
//~ says: requires `backpressure`
use trame::require;

require!(backpressure);
