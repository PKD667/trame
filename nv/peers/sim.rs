//! The host model of the launch's peers: one `Mesh` of `Link`s, shared by the participants of a
//! launch and owned by whoever built the environment.
//!
//! The mesh is behind an `Arc` because the model's participants really do share one fabric — that
//! is what makes them able to reach each other — while each participant's [`Links`] value holds
//! only its own endpoints. That is the same division as the device's: the fabric is shared, the
//! endpoints are per participant.

use std::sync::Arc;

use super::{Refused, refused_recv, refused_send};
use crate::nv::layout::Layout;
use crate::nv::transport::Message;
use crate::nv::transport::Transport;
use crate::nv::transport::sim::{Mesh, SimTransport};

/// What a launch supplies for its links to exist: the model's shared mesh and its geometry.
///
/// Ordinary allocation, so constructing one is safe. There is no arena contract to uphold because
/// there is no device memory and no raw pointer.
#[derive(Clone)]
pub struct Fabric {
    mesh: Arc<Mesh>,
    layout: Layout,
}

impl Fabric {
    pub fn new(size: u32, layout: Layout) -> Self {
        Fabric {
            mesh: Arc::new(Mesh::new(size, layout)),
            layout,
        }
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }
}

/// One participant's endpoints. Participant-local: nothing here is shared with another, which is
/// what makes it sound to hold behind a `&mut`.
pub struct Links {
    rank: u32,
    size: u32,
    layout: Layout,
    transport: SimTransport,
}

impl Links {
    /// Open this participant's endpoints over the launch's fabric.
    ///
    /// `rank >= size` is refused rather than clamped: a rank outside the launch has no row in the
    /// mesh, and a clamped rank would send to the wrong participant.
    pub fn open(fabric: &Fabric, rank: u32, size: u32) -> Result<Links, Refused> {
        if rank >= size {
            return Err(Refused::NoSuchPeer);
        }
        Ok(Links {
            rank,
            size,
            layout: fabric.layout,
            transport: fabric.mesh.transport(rank),
        })
    }

    pub fn rank(&self) -> u32 {
        self.rank
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn send(&mut self, dest: u32, tag: u32, data: &[u8]) -> Result<(), Refused> {
        if dest >= self.size {
            return Err(Refused::NoSuchPeer);
        }
        self.transport
            .try_send(dest, tag, data)
            .map_err(refused_send)
    }

    pub fn recv(&mut self, src: u32, out: &mut [u8]) -> Result<Message, Refused> {
        if src >= self.size {
            return Err(Refused::NoSuchPeer);
        }
        self.transport.try_recv(src, out).map_err(refused_recv)
    }
}
