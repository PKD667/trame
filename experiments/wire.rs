// What an MPI experiment needs in order to drive the backend, once.
//
// One file included by every experiment, so each of them sees only the methods it calls and the
// rest are dead as far as that build is concerned. That is a property of sharing the file rather
// than of the methods being unused, which is what the allow says.
#![allow(dead_code)]
//
// The contract moved three obligations to the caller: the environment is a value the caller owns
// rather than a global, a receive writes into a buffer the caller supplies, and demultiplexing by
// tag is the caller's job — there is no tagged receive and no backend stash. Every experiment owes
// the same three, so they are paid once here rather than four times.
//
// It is deliberately small and it is *not* the old surface restored. There is no `Inflight`, no
// `process::exit` and no global: what is here is a context, a held-frame list the caller must keep
// because the backend no longer does, and the four calls the experiments make.
//
// Ranks and tags are plain numbers here because the experiments do arithmetic on them; they become
// the contract's types at each call. Every backend call is one attempt, so the waiting an
// experiment wants is `until`'s, and only there.

use trame::{Deployment, init, rank, recv, send, size};

/// The contract's value types, so an experiment names one module rather than two.
pub use trame::{Channel, Context, Environment, Error};

pub type Rank = u32;
pub type Tag = u16;

/// Where the launch states how many workers it started. One more process, launch rank
/// `TRAME_WORKERS`, is their leader, and says so with `--leader` in argv.
const WORKERS: &str = "TRAME_WORKERS";

/// Repeat a one-attempt call while it reports nothing yet, `Full` or `Busy`.
fn until<T>(mut attempt: impl FnMut() -> Result<Option<T>, Error>) -> Result<T, Error> {
    loop {
        match attempt() {
            Ok(Some(value)) => return Ok(value),
            Ok(None) | Err(Error::Full) | Err(Error::Busy) => core::hint::spin_loop(),
            Err(error) => return Err(error),
        }
    }
}

/// The workers the launch stated in `TRAME_WORKERS`, as launch ranks. The contract ranks are the
/// positions.
fn workers() -> Vec<trame::Launch> {
    let stated = std::env::var(WORKERS).unwrap_or_else(|e| panic!("{WORKERS}: {e}"));
    let n: u32 = stated
        .parse()
        .unwrap_or_else(|_| panic!("{WORKERS}: `{stated}` is not a count"));
    (0..n).map(trame::Launch::new).collect()
}

/// One participant, and the frames it has received but not yet been asked for.
pub struct Wire {
    cx: Context,
    /// Frames whose tag nobody has asked about yet. A link has no peek, so an ask about one tag
    /// that meets a frame under another has to put it somewhere, and the contract says that
    /// somewhere is here.
    held: Vec<(Rank, Tag, Vec<u8>)>,
    /// The receive buffer, grown to the largest frame seen. An experiment's traffic is its own,
    /// so this converges on the first oversized frame rather than growing per call.
    cap: usize,
}

impl Wire {
    /// Enter the world. The experiments use point-to-point only, so the cohort is never read.
    ///
    /// The process started with `--leader` is the deployment's leader. It carries nothing: it
    /// opens the route every deployment has, holds it until the workers finish, and leaves.
    pub fn start() -> Wire {
        let workers = workers();
        let hosts: [&[trame::Launch]; 1] = [&workers];
        let leader = trame::Launch::new(workers.len() as u32);
        let deployment = Deployment::new(&hosts, 0, leader).expect("a stated deployment");
        if std::env::args().any(|arg| arg == "--leader") {
            let mut route = trame::leader::Leader::open(Environment::default(), deployment)
                .expect("this experiment needs MPI");
            route.done::<core::convert::Infallible>(Ok(())).expect("finalize leader");
            std::process::exit(0);
        }
        Wire {
            cx: init(Environment::default(), deployment).expect("this experiment needs MPI"),
            held: Vec::new(),
            cap: 64,
        }
    }

    /// One attempt, with the refusal returned rather than turned into a panic.
    pub fn try_post(&mut self, dest: Rank, tag: Tag, data: &[u8]) -> Result<(), Error> {
        let dest = trame::Addr::Local(dest);
        send(&mut self.cx, dest, Channel::Message(trame::Tag::new(tag)), data)
    }

    pub fn rank(&self) -> Rank {
        rank(&self.cx)
    }

    pub fn size(&self) -> u32 {
        size(&self.cx)
    }

    /// One attempt that must be accepted.
    pub fn post(&mut self, dest: Rank, tag: Tag, data: &[u8]) {
        self.try_post(dest, tag, data)
            .unwrap_or_else(|e| panic!("post to {dest}: {e}"));
    }

    /// Repeated until accepted: for a frame refused with `Full`, and never in a
    /// symmetric exchange where both peers send before either receives.
    pub fn put(&mut self, dest: Rank, tag: Tag, data: &[u8]) {
        until(|| self.try_post(dest, tag, data).map(Some))
            .unwrap_or_else(|e| panic!("put to {dest}: {e}"));
    }

    /// One frame to each listed peer. A loop over `post`, because a broadcast would need every
    /// peer to call it at the same point and these peers are in receive loops.
    pub fn broadcast(&mut self, dests: &[Rank], tag: Tag, data: &[u8]) {
        for &dest in dests {
            self.post(dest, tag, data);
        }
    }

    /// The next frame under `want`'s tag, holding whatever arrives under another.
    ///
    /// Repeated until a frame arrives, because an experiment knows its own traffic and is not
    /// measuring the wait. Nothing accepted is ever dropped: what was not asked for is kept for
    /// the ask that comes.
    pub fn take(&mut self, want: Tag) -> (Rank, Vec<u8>) {
        if let Some(at) = self.held.iter().position(|(_, tag, _)| *tag == want) {
            let (from, _, data) = self.held.remove(at);
            return (from, data);
        }
        loop {
            match until(|| self.receive()).unwrap_or_else(|e| panic!("recv: {e}")) {
                (from, tag, data) if tag == want => return (from, data),
                frame => self.held.push(frame),
            }
        }
    }

    /// Report the run's outcome and finalize. It returns rather than exiting: the entry rules make
    /// process exit non-portable, so an experiment that is done is an experiment that returns from
    /// `main`. Leave the run. It returns, so a caller that means to stop says so: the old `done`
    /// exited the process, which the entry rules make non-portable, and a call that looks like a
    /// stop but is not one is exactly the kind of promise this contract stopped making.
    pub fn done(&mut self) {
        trame::done::<core::convert::Infallible>(&mut self.cx, Ok(())).expect("finalize");
    }

    /// The next frame, whatever its tag, or nothing if none is waiting.
    pub fn poll(&mut self) -> Option<(Rank, Tag, Vec<u8>)> {
        if !self.held.is_empty() {
            return Some(self.held.remove(0));
        }
        self.receive().unwrap_or_else(|e| panic!("recv: {e}"))
    }

    /// One receive into the buffer, growing it when the backend says the frame is larger.
    fn receive(&mut self) -> Result<Option<(Rank, Tag, Vec<u8>)>, Error> {
        loop {
            let mut buf = vec![0u8; self.cap];
            match recv(&mut self.cx, &mut buf) {
                Ok(Some(frame)) => {
                    buf.truncate(frame.len());
                    let source = match frame.source() {
                        Some(trame::Addr::Local(source)) => source,
                        other => panic!("a peer frame names a sender of this deployment, not {other:?}"),
                    };
                    return Ok(Some((source, frame.tag().get(), buf)));
                }
                Ok(None) => return Ok(None),
                // The refusal consumed nothing, so asking again with a larger buffer is the whole
                // fix and the frame is still there.
                Err(Error::TooSmall { needed }) => self.cap = needed,
                Err(e) => return Err(e),
            }
        }
    }
}
