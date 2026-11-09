//! What a step returns: INTERFACE.md 5.3, Definition (Status). `Progress` and `Done` carry no condition.
//! `Status::wait()` is wait(∅), and each builder method adds one condition to it. Any status but
//! `Done` may also carry a weight for the step's next start (`.weight(w)`, sched.rs). The scheduler
//! takes the conditions as flags per step, so this file only builds them.

use crate::backend::{Backend, Selected};
use crate::io::Message;
use crate::sched::Weight;
use crate::time::Reading;
use core::any::{TypeId, type_name};

/// The status a step returns (INTERFACE.md 5.3). A failing step answers `Err(e)` from a step that returns
/// `Result<Status, E>`, which is the spec's `err(e)`. Build it with `Status::Progress`, `Status::Done`
/// or `Status::wait()`, and optionally `.weight(w)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    kind: Kind,
    weight: Option<Weight>,
}

/// What a status says about the step's next start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The step did work and can run again.
    Progress,
    /// The step is finished. It is not run again.
    Done,
    /// The step waits for the conditions in `Waits`.
    Wait(Waits),
}

#[allow(non_upper_case_globals)]
impl Status {
    /// The step did work and can run again.
    pub const Progress: Status = Status { kind: Kind::Progress, weight: None };
    /// The step is finished. It is not run again.
    pub const Done: Status = Status { kind: Kind::Done, weight: None };
}

/// The conditions Ω of a wait. Users never name this type: they build it with `Status::wait()`, then
/// `.frame()`, `.room::<M>(to)` and `.until(reading)`. Any one holding wakes the process, and so does
/// a step that shares one of its cells ending. With no condition, only that wakes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Waits {
    /// A frame of a type this process owns is available to this worker.
    frame: bool,
    /// Bit `k` set: the outgoing route `k` of this worker (in plan order) has room, or is lossy.
    rooms: u64,
    /// The earliest deadline: this worker's clock has reached it. Several deadlines keep the earliest,
    /// since the wait holds when any does.
    until: Option<Reading>,
}

impl Waits {
    pub(crate) const fn frame_wanted(self) -> bool {
        self.frame
    }

    pub(crate) const fn room_mask(self) -> u64 {
        self.rooms
    }

    pub(crate) const fn deadline(self) -> Option<Reading> {
        self.until
    }
}

impl Status {
    /// The wait with no condition (INTERFACE.md 5.3, wait(∅)): only the end of a step that writes a field
    /// this step takes wakes it. Add conditions with the methods below.
    pub const fn wait() -> Status {
        Status { kind: Kind::Wait(Waits { frame: false, rooms: 0, until: None }), weight: None }
    }

    /// Set the step's weight in [0, 1] from its next start on (sched.rs): under contention, steps of
    /// positive weight share the worker in proportion to their weights, and a step of weight 0 runs only
    /// when no step of positive weight can. Panics outside [0, 1] or on `Done`, which has no next start.
    pub fn weight(self, w: f64) -> Status {
        assert!(self.kind != Kind::Done, "trame: .weight on Status::Done: a done step does not start again");
        Status { weight: Some(Weight::from_f64(w)), ..self }
    }

    pub(crate) const fn kind(self) -> Kind {
        self.kind
    }

    pub(crate) const fn new_weight(self) -> Option<Weight> {
        self.weight
    }

    /// Add the condition: a frame of a type this process owns is available to this worker.
    pub fn frame(self) -> Status {
        self.with_condition(".frame()", |w| Waits { frame: true, ..w })
    }

    /// Add the condition: the route from this worker to `to`, carrying `M`, has room for a send. Aborts
    /// when this worker has no such route, since the condition could never change.
    pub fn room<M: Message>(self, to: impl Into<crate::topology::End>) -> Status {
        let to = to.into();
        self.with_condition(".room()", |w| {
            let route = Selected::worker_route(to, TypeId::of::<M>()).unwrap_or_else(|| {
                panic!("trame: .room::<{}>({to:?}) names no route from this worker of that type; declare the route in the topology", type_name::<M>())
            });
            assert!(route < 64, "trame: .room waits on route {route}, but a worker can wait on at most 64 routes");
            Waits { rooms: w.rooms | 1 << route, ..w }
        })
    }

    /// Add the condition: this worker's clock has reached `at`. `at` must be a reading of this worker's
    /// clock; a reading of another clock aborts the run when the step returns (concurrent.rs). Readings
    /// of two clocks in one wait abort here.
    pub fn until(self, at: Reading) -> Status {
        self.with_condition(".until()", |w| {
            let until = match w.until {
                None => at,
                Some(prev) => {
                    assert!(
                        prev.clock() == at.clock(),
                        "trame: .until was given readings of two clocks in one wait; a wait names one clock"
                    );
                    if at.at() < prev.at() { at } else { prev }
                }
            };
            Waits { until: Some(until), ..w }
        })
    }

    /// Apply a condition to a wait. Only `Status::wait()` and its methods carry conditions, so any other
    /// status aborts here, naming the method and the status.
    fn with_condition(self, method: &str, add: impl FnOnce(Waits) -> Waits) -> Status {
        match self.kind {
            Kind::Wait(w) => Status { kind: Kind::Wait(add(w)), ..self },
            Kind::Progress => {
                panic!("trame: {method} adds a wait condition, but this Status is Progress; start from Status::wait()")
            }
            Kind::Done => {
                panic!("trame: {method} adds a wait condition, but this Status is Done; start from Status::wait()")
            }
        }
    }
}
