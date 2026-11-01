// The small check: the path of small cross-node frames (RMA.md). Workers 0 and 1 send on three routes:
//   route 0: 0 -> 1, 16 bytes (small)      route 1: 1 -> 0, 200 bytes (small)
//   route 2: 0 -> 1, FRAME bytes (large, p2p)
// Every frame is checked (route, FIFO, payload). Then the number of small frames this process's relay
// sent by lane is compared with what it should be. That is M per worker when TRAME_MPI_SMALL=rma and
// TRAME_MPI_NODE_RANKS=1 (each worker is then a node of its own, so its small frames are cross-node),
// and 0 otherwise. The count is read by the caller after finalization, when the relay has stopped.

#![allow(unused_imports)]

use crate::common::*;
use mpi::traits::*;

use trame::__mpi::{Answer, Received, Role, Route, RouteId, Topology, launch, leader_join, returned};

const SMALL: [usize; 2] = [16, 200];

/// Returns the lane count this process must show: M for a worker under the rma split, else 0.
pub(crate) fn small(world: &mpi::topology::SimpleCommunicator, args: &[String]) -> u64 {
    let m = arg_num(args, "--msgs", 300);
    let me = world.rank() as u32;
    let topo = Topology {
        workers: 2,
        routes: vec![
            Route { from: 0, to: 1, tag: 0, size: SMALL[0], cap: 4, lossy: false },
            Route { from: 1, to: 0, tag: 0, size: SMALL[1], cap: 4, lossy: false },
            Route { from: 0, to: 1, tag: 1, size: FRAME, cap: 2, lossy: false },
        ],
        publication: b"init".to_vec(),
    };
    let mut t = match worker(world, &topo) {
        Ok(t) => t,
        Err(l) => {
            leader_join(&l);
            return 0;
        }
    };
    // Each worker sends M on its small route; worker 0 also sends M large frames on route 2.
    let tokens: Vec<u32> = match me {
        0 => (0..m).flat_map(|_| [0u32, 2u32]).collect(),
        _ => (0..m).map(|_| 1u32).collect(),
    };
    let owned = ids_to(&topo.routes, me);
    // Worker 0 receives route 1; worker 1 receives routes 0 and 2.
    let expected_in = if me == 0 { m } else { 2 * m };
    let mut sent_seq = [0u64; 3];
    let mut recv_seq = [0u64; 3];
    let mut next = 0usize;
    let mut received = 0u64;
    let mut msg = vec![0u8; FRAME];
    let mut buf = vec![0u8; FRAME];
    while next < tokens.len() || received < expected_in {
        if next < tokens.len() {
            let r = tokens[next];
            let size = topo.routes[r as usize].size;
            fill(r, sent_seq[r as usize], &mut msg[..size]);
            if t.send(RouteId(r), &msg[..size]).expect("send") == Answer::Ok {
                sent_seq[r as usize] += 1;
                next += 1;
            }
        }
        if let Received::Message(id) = t.recv(&owned, &mut buf).expect("recv") {
            let r = id.0 as usize;
            check_frame(id.0, recv_seq[r], &buf[..topo.routes[r].size]);
            recv_seq[r] += 1;
            received += 1;
        }
    }
    println!("small rank {me}: sent {} received {received}", tokens.len());
    returned(&t);
    let rma_split = std::env::var("TRAME_MPI_SMALL").as_deref() == Ok("rma")
        && std::env::var("TRAME_MPI_NODE_RANKS").as_deref() == Ok("1");
    if rma_split { m } else { 0 }
}
