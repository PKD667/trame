//! The host transport: every rank a thread over one mesh of modelled links.

use crate::nv::error::SendError;
use crate::nv::layout::Layout;
use crate::nv::transport::{Transport, sim};

const PTAG: u32 = 3;
const PLEN: usize = 24;

#[test]
fn pingpong_round_trips() {
    let layout = Layout::new(2, 64).expect("layout");
    let mesh = sim::Mesh::new(2, layout);
    let results = mesh.spawn(|mut tr| {
        let other = 1 - tr.rank();
        let mut payload = [0u8; PLEN];
        let mut got = [0u8; PLEN];
        let mut tags = Vec::new();
        for i in 0..1000u32 {
            payload.fill((i % 251) as u8);
            tr.send(other, PTAG + i, &payload).expect("blocking send");
            let msg = tr.recv(other, &mut got).expect("blocking recv");
            assert_eq!(msg.tag, PTAG + i);
            assert!(got.iter().all(|&b| b == (i % 251) as u8));
            tags.push(msg.tag);
        }
        tags
    });
    assert_eq!(results[0], results[1]);
}

#[test]
fn ring_full_is_reported() {
    let layout = Layout::new(2, 64).expect("layout");
    let mesh = sim::Mesh::new(1, layout);
    let mut tr = mesh.transport(0);
    let payload = [7u8; PLEN];
    tr.try_send(0, 0, &payload).expect("first frame");
    tr.try_send(0, 0, &payload).expect("second frame");
    assert!(matches!(tr.try_send(0, 0, &payload), Err(SendError::Full)));
}
