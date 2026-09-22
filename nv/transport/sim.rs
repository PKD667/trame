//! Host transport: each rank is a thread over a `model::Link` mesh.

use std::sync::{Arc, Mutex};

use super::Transport;
use crate::contract::{Rank, Tag};
use crate::nv::error::{RecvError, SendError};
use crate::nv::layout::Layout;
use crate::nv::model::Link;
use crate::nv::transport::Message;

struct Links {
    size: Rank,
    links: Vec<Mutex<Link>>,
}

impl Links {
    fn link(&self, src: Rank, dst: Rank) -> &Mutex<Link> {
        &self.links[(src * self.size + dst) as usize]
    }
}

/// `size × size` directed links, one per (src, dst) pair.
pub struct Mesh {
    inner: Arc<Links>,
}

impl Mesh {
    pub fn new(size: Rank, layout: Layout) -> Self {
        let links = (0..size * size)
            .map(|_| Mutex::new(Link::new(layout)))
            .collect();
        Self {
            inner: Arc::new(Links { size, links }),
        }
    }

    pub fn size(&self) -> Rank {
        self.inner.size
    }

    pub fn transport(&self, rank: Rank) -> SimTransport {
        SimTransport {
            rank,
            inner: Arc::clone(&self.inner),
        }
    }

    /// Runs `f` on one thread per rank, returning one result per rank.
    pub fn spawn<R: Send>(&self, f: impl Fn(SimTransport) -> R + Send + Sync) -> Vec<R> {
        let f = Arc::new(f);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..self.inner.size)
                .map(|rank| {
                    let inner = Arc::clone(&self.inner);
                    let f = Arc::clone(&f);
                    scope.spawn(move || f(SimTransport { rank, inner }))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("rank thread panicked"))
                .collect()
        })
    }
}

/// One rank's view of a `Mesh`.
pub struct SimTransport {
    rank: Rank,
    inner: Arc<Links>,
}

impl Transport for SimTransport {
    fn rank(&self) -> Rank {
        self.rank
    }

    fn size(&self) -> Rank {
        self.inner.size
    }

    fn try_send(&mut self, dst: Rank, tag: Tag, data: &[u8]) -> Result<(), SendError> {
        self.inner
            .link(self.rank, dst)
            .lock()
            .expect("link poisoned")
            .send(self.rank, tag, data)
    }

    fn try_recv(&mut self, src: Rank, out: &mut [u8]) -> Result<Message, RecvError> {
        self.inner
            .link(src, self.rank)
            .lock()
            .expect("link poisoned")
            .recv(out)
    }
}
