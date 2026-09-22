// One participant, so there is no collective to join. The copy exists before anything reads it,
// which is `prepublished` and is not `collective`.
//~ says: requires `collective_share`
use trame::require;

require!(collective_share);
