//! Stand-in for a build that selects no backend module (the `mpi` feature, until it has one). It
//! refuses every launch, so no worker starts and no other operation can be reached.

use crate::backend::{Backend, PoolRef};
use crate::sched::{Event, Sched};
use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::run::Outcome;
use crate::time::ClockId;
use crate::topology::WorkerId;
use alloc::vec::Vec;
use core::any::TypeId;

pub struct Selected;

/// Uninhabited: `launch` never returns one.
pub enum Run {}

impl Backend for Selected {
    type Run = Run;

    fn launch(_plan: Plan) -> Result<Run, Refused> {
        Err(Refused::Backend { reason: "no backend selected" })
    }

    fn worker_send(_to: WorkerId, _ty: TypeId, _name: &'static str, _bytes: &[u8]) -> Result<(), Full> {
        unreachable!("no worker starts without a backend")
    }

    fn worker_recv(_ty: TypeId, _out: &mut [u8]) -> bool {
        unreachable!("no worker starts without a backend")
    }

    fn worker_offer(_ty: TypeId) -> bool {
        unreachable!("no worker starts without a backend")
    }

    fn worker_route(_to: WorkerId, _ty: TypeId) -> Option<usize> {
        unreachable!("no worker starts without a backend")
    }

    fn worker_room(_route: usize) -> bool {
        unreachable!("no worker starts without a backend")
    }

    fn worker_epoch() -> u64 {
        unreachable!("no worker starts without a backend")
    }

    fn worker_idle(_until: Option<u64>, _epoch: u64) {
        unreachable!("no worker starts without a backend")
    }

    fn worker_now() -> (ClockId, u64) {
        unreachable!("no worker starts without a backend")
    }

    fn worker_me() -> WorkerId {
        unreachable!("no worker starts without a backend")
    }

    fn worker_abandon() -> ! {
        unreachable!("no worker starts without a backend")
    }

    fn leader_send(run: &Run, _to: WorkerId, _ty: TypeId, _name: &'static str, _bytes: &[u8]) -> Result<(), Full> {
        match *run {}
    }

    fn leader_recv(run: &Run, _ty: TypeId, _out: &mut [u8]) -> bool {
        match *run {}
    }

    fn leader_pool(run: &Run, _pool: PoolRef) {
        match *run {}
    }

    fn leader_edit(run: &Run, _w: usize, _f: &mut dyn FnMut()) -> bool {
        match *run {}
    }

    fn leader_stop(run: &Run) {
        match *run {}
    }

    fn leader_join_traced(run: Run, _workers: usize) -> (Vec<Outcome>, Vec<Option<(Sched, Vec<Event>)>>) {
        match run {}
    }

    fn leader_wait(run: &Run, _seen: u64) -> u64 {
        match *run {}
    }

    fn leader_join(run: Run) -> Vec<Outcome> {
        match run {}
    }
}
