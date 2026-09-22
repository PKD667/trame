// The host mechanisms, one module per subject. They live under `cpu/` because every one of them
// is a fact about a host process — a thread to start, a clock to read, a mutex to take — and not
// about the backend contract the transport surface states.

mod clock;
