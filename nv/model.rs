//! Deterministic executable model of one directed link.

use crate::contract::{Rank, Tag};
use crate::nv::error::{RecvError, SendError};
use crate::nv::layout::Layout;
use crate::nv::transport::Message;

pub struct Link {
    layout: Layout,
    pub arena: Vec<u32>,
    pub send: u32,
    pub recv: u32,
}

impl Link {
    pub fn new(layout: Layout) -> Self {
        let mut arena = vec![0; layout.words()];
        layout.init(&mut arena);
        Self {
            layout,
            arena,
            send: 0,
            recv: 0,
        }
    }

    pub fn send(&mut self, src: Rank, tag: Tag, data: &[u8]) -> Result<(), SendError> {
        if data.len() > self.layout.capacity() as usize {
            return Err(SendError::TooLarge);
        }
        let slot = self.layout.slot(self.send);
        if self.arena[slot] != self.send {
            return Err(SendError::Full);
        }

        self.arena[slot + 1] = data.len() as u32;
        self.arena[slot + 2] = src;
        self.arena[slot + 3] = tag;
        let payload = &mut self.arena[slot + 4..slot + self.layout.slot_words()];
        payload.fill(0);
        for (i, byte) in data.iter().enumerate() {
            payload[i / 4] |= (*byte as u32) << ((i % 4) * 8);
        }
        self.arena[slot] = self.send.wrapping_add(1);
        self.send = self.send.wrapping_add(1);
        Ok(())
    }

    pub fn recv(&mut self, out: &mut [u8]) -> Result<Message, RecvError> {
        let slot = self.layout.slot(self.recv);
        if self.arena[slot] != self.recv.wrapping_add(1) {
            return Err(RecvError::Empty);
        }
        let len = self.arena[slot + 1];
        if out.len() < len as usize {
            return Err(RecvError::TooSmall { needed: len });
        }

        let message = Message {
            len,
            src: self.arena[slot + 2],
            tag: self.arena[slot + 3],
        };
        let payload = &self.arena[slot + 4..slot + self.layout.slot_words()];
        for i in 0..len as usize {
            out[i] = (payload[i / 4] >> ((i % 4) * 8)) as u8;
        }
        self.arena[slot] = self.recv.wrapping_add(self.layout.depth());
        self.recv = self.recv.wrapping_add(1);
        Ok(message)
    }
}
