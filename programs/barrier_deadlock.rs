//! barrier_deadlock: worker 0 returns at once. Workers 1 and 2 call the barrier, which can never
//! complete, because worker 0 will never arrive (spec.md 5, Deadlock: a worker waiting at a barrier
//! while another worker has a result is stuck, whatever anyone does). Both waiters end with Deadlock.
//! The leader's join answers the three outcomes in declaration order.

use trame::{Context, Outcome, Topology};

fn quitter(_cx: &Context) {}

fn waiter(_cx: &Context) {
    // Never completes: the barrier unwinds this worker with Deadlock.
    trame::barrier();
    unreachable!("a barrier with a returned worker completed");
}

fn main() {
    let mut t = Topology::new();
    t.worker(quitter);
    t.worker(waiter);
    t.worker(waiter);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}
