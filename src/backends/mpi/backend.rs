// The Backend trait on the MPI transport (CONTRACT.md). `launch` hands a worker rank to roles/runner.rs, which
// never returns, and the leader rank a `Leader` (roles/leader.rs). The rest forwards to roles/worker.rs,
// roles/leader.rs, memory/segment.rs and transport/hosts.rs.

extern crate std;

use core::any::TypeId;
use mpi::traits::Communicator as _;
use std::vec::Vec;

use crate::backend::{Backend, Target};
use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::run::Outcome;
use crate::sched::{Event, Sched};
use crate::time::ClockId;
use crate::vocab::{End, WorkerId};

use crate::backends::mpi::Selected;
use crate::backends::mpi::memory::segment;
use crate::backends::mpi::roles::leader::{self, Leader};
use crate::backends::mpi::roles::runner::{agents_for, run_worker};
use crate::backends::mpi::roles::worker;
use crate::backends::mpi::setup::adopt::build_or_adopt;
use crate::backends::mpi::setup::open::{agree, open};
use crate::backends::mpi::setup::plan::{encode_plan, entries, inbox, topology};
use crate::backends::mpi::setup::state::{local, refused, world};
use crate::backends::mpi::types::Role;

impl Backend for Selected {
    type Run = Leader;

    /// MPI_Init and MPI_Finalize are once per process, and a worker rank exits with its run.
    const RUNS_PER_PROCESS: usize = 1;

    /// The ranks of the host agree on the plan, then the hosts on the routes between them (F3,
    /// cross/connect.rs `connect`). The launch checks run here too, so that a host refused by them still
    /// takes part in the connect, and the hosts it shares routes with are refused with it.
    fn agree(plan: &Plan) -> Result<(), Refused> {
        let ranks = plan.workers.len() as u32 + 1;
        // `trame run` (crates/cli/trame) asks how many ranks the program needs: TRAME_PROBE=ranks prints this host's
        // count and stops, on one rank, before any collective, so a world of one rank is enough to ask.
        // With TRAME_PROBE_OUT the count goes to that file: a test harness captures stdout (libtest does), and
        // a count printed into its capture is lost when the process exits here.
        if std::env::var("TRAME_PROBE").as_deref() == Ok("ranks") {
            match std::env::var_os("TRAME_PROBE_OUT") {
                Some(path) => {
                    std::fs::write(path, std::format!("trame-ranks {ranks}\n")).expect("TRAME_PROBE_OUT is writable")
                }
                None => std::println!("trame-ranks {ranks}"),
            }
            crate::backends::mpi::setup::state::finalize();
            std::process::exit(0);
        }
        let here =
            agree(&crate::backends::mpi::transport::hosts::host().comm, &encode_plan(plan), ranks).map_err(refused);
        let checked = here.and_then(|()| crate::launch::check(plan));
        let joined = crate::backends::mpi::transport::cross::connect(plan, checked.is_ok());
        checked.and(joined)
    }

    fn launch(mut plan: Plan) -> Result<Leader, Refused> {
        local().launched = true;
        // The debug dump (docs/debug.md): the arena boots on every rank, worker and leader, named
        // by the rank's world rank and its host. Empty when the cfg is off.
        crate::debug::boot(
            u32::try_from(world().rank()).unwrap_or(0),
            crate::backends::mpi::transport::hosts::host().id,
        );
        let topo = topology(&plan)?;
        let role = open(&crate::backends::mpi::transport::hosts::host().comm, &topo).map_err(refused)?;
        let t = match &role {
            Role::Leader(t) | Role::Worker(t) => t,
        };
        build_or_adopt(&mut plan, t)?;
        let table = entries(&plan);
        let l = local();
        l.entries = table;
        match role {
            Role::Leader(t) => {
                l.inbox = inbox(&l.entries, t.me());
                crate::backends::mpi::roles::detect::watch(&t);
                Ok(Leader::new(t, (core::mem::take(&mut plan.workers), plan.pool.take())))
            }
            Role::Worker(t) => {
                let me = t.me();
                l.inbox = inbox(&l.entries, me);
                let obj = plan.workers[me as usize].clone();
                let agents = agents_for(&*obj, &t, plan.agents);
                l.endpoint = Some(t);
                run_worker(obj, agents, plan.pool.clone(), plan.trace)
            }
        }
    }

    fn worker_send(to: End, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        worker::send(to, ty, name, bytes)
    }

    fn worker_recv(ty: TypeId, out: &mut [u8]) -> bool {
        worker::recv(ty, out)
    }

    fn worker_offer(ty: TypeId) -> bool {
        worker::offer(ty)
    }

    fn worker_route(to: End, ty: TypeId) -> Option<usize> {
        worker::route_index(to, ty)
    }

    fn worker_incoming(ty: TypeId) -> Option<usize> {
        worker::incoming(ty)
    }

    fn worker_room(route: usize) -> bool {
        worker::room(route)
    }

    fn worker_epoch() -> u64 {
        worker::epoch()
    }

    fn worker_idle(epoch: u64) {
        worker::idle(epoch)
    }

    fn worker_mark_start() {
        worker::mark_start()
    }

    fn worker_mark_end() {
        worker::mark_end()
    }

    fn worker_mark_quiet(epoch: u64) {
        worker::mark_quiet(epoch)
    }

    fn worker_now() -> (ClockId, u64) {
        worker::now()
    }

    fn worker_abandon() -> ! {
        std::panic::resume_unwind(std::boxed::Box::new(worker::Aborted))
    }

    fn worker_me() -> WorkerId {
        WorkerId::here(worker::me())
    }

    fn worker_stop_requested() -> bool {
        worker::stop_requested()
    }

    fn leader_send(run: &Leader, to: WorkerId, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        leader::send(run, to, ty, name, bytes)
    }

    fn leader_recv(run: &Leader, ty: TypeId, out: &mut [u8]) -> bool {
        leader::recv(run, ty, out)
    }

    fn leader_edit(run: &Leader, target: Target, f: &mut dyn FnMut()) -> bool {
        leader::edit(run, target, f)
    }

    fn leader_stop(run: &Leader) {
        leader::stop(run)
    }

    fn leader_join_traced(run: Leader, workers: usize) -> (Vec<Outcome>, Vec<Option<(Sched, Vec<Event>)>>) {
        let here = leader::local_workers(&run);
        let out = leader::join(run);
        let traces = (0..workers as u32)
            .map(|w| {
                let at = segment::trace_slot(w).load(std::sync::atomic::Ordering::Acquire);
                (here.contains(&w) && at != 0).then(|| {
                    // SAFETY: the worker wrote a boxed trace there before its result, and only this
                    // rank reads it, once (CONTRACT.md, S9).
                    *unsafe { std::boxed::Box::from_raw(at as *mut (Sched, Vec<Event>)) }
                })
            })
            .collect();
        (out, traces)
    }

    fn worker_edit_request() -> u64 {
        worker::edit_request()
    }

    fn worker_edit_held(held: bool) {
        worker::edit_held(held)
    }

    fn domain_alloc(layout: core::alloc::Layout) -> Option<*mut u8> {
        segment::mapped().then(|| segment::alloc(layout))
    }

    fn has_domain() -> bool {
        segment::mapped()
    }

    fn domain_contains(p: *mut u8) -> bool {
        segment::contains(p)
    }

    unsafe fn domain_dealloc(p: *mut u8, layout: core::alloc::Layout) {
        segment::free(p, layout)
    }

    fn leader_wait(run: &Leader, seen: u64) -> u64 {
        leader::wait(run, seen)
    }

    fn host() -> u32 {
        // The launcher asks the program its rank count once per host, as host TRAME_PROBE_HOST of TRAME_PROBE_HOSTS.
        if std::env::var("TRAME_PROBE").as_deref() == Ok("ranks") {
            if let Some(h) = std::env::var("TRAME_PROBE_HOST").ok().and_then(|v| v.parse().ok()) {
                return h;
            }
        }
        crate::backends::mpi::transport::hosts::host().id
    }

    fn hosts() -> u32 {
        // The launcher (launch) asks a program its rank count on one rank, as a host of TRAME_PROBE_HOSTS (host()).
        if std::env::var("TRAME_PROBE").as_deref() == Ok("ranks") {
            if let Some(h) = std::env::var("TRAME_PROBE_HOSTS").ok().and_then(|v| v.parse().ok()) {
                return h;
            }
        }
        crate::backends::mpi::transport::hosts::host().count
    }

    fn workers(h: u32) -> Option<u32> {
        // The launcher's probe is one rank standing for a host of a larger launch: its counts are the supplied ones.
        if std::env::var("TRAME_PROBE").as_deref() == Ok("ranks") {
            return crate::backends::supply::supplied(h);
        }
        // A host's workers are its ranks but its leader, known once the hosts have discovered each other (hosts.rs
        // `host`). Until the launch begins the supplied count answers first: it is what the launcher asked the
        // program for its ranks on, so a program that derives its own count from it declares what the launch holds.
        let local = crate::backends::mpi::setup::state::local();
        let held = || local.host.as_ref()?.members.get(h as usize).map(|ranks| ranks.len() as u32 - 1);
        if local.launched { held() } else { crate::backends::supply::supplied(h).or_else(held) }
    }

    fn host_available() -> Option<u64> {
        crate::backends::supply::available()
    }

    fn is_leader() -> bool {
        use mpi::topology::Communicator;
        let comm = &crate::backends::mpi::transport::hosts::host().comm;
        comm.rank() == comm.size() - 1
    }

    fn share(bytes: &[u8]) -> Vec<u8> {
        crate::backends::mpi::transport::hosts::share(bytes)
    }

    fn peer_send(to: u32, tag: u64, bytes: &[u8]) -> Result<(), Full> {
        no_probe("peer_send");
        crate::backends::mpi::transport::hosts::send(to, tag, bytes);
        Ok(())
    }

    fn peer_wait() {
        no_probe("peer_wait");
        crate::backends::mpi::transport::hosts::wait();
    }

    fn peer_recv(tag: u64, out: &mut [u8]) -> Option<u32> {
        no_probe("peer_recv");
        crate::backends::mpi::transport::hosts::recv(tag, out)
    }

    fn leader_join(run: Leader) -> Vec<Outcome> {
        leader::join(run)
    }
}

/// The launcher's probe is one rank that stands for a host, so it has no peers: a program that exchanges messages
/// before its launch is given its workers (`launch --workers`), and the launcher does not probe it.
fn no_probe(what: &str) {
    assert!(
        std::env::var("TRAME_PROBE").as_deref() != Ok("ranks"),
        "trame: {what} before a launch needs the workers given to the launcher (launch --workers N,N,...), not asked of the program"
    );
}
