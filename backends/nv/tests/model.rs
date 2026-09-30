use crate::nv::error::{RecvError, SendError};
use crate::nv::layout::Layout;
use crate::nv::model::Link;
use crate::nv::transport::Message;

#[test]
fn roundtrip_preserves_metadata_and_unaligned_payloads() {
    for len in [0, 1, 3, 4, 5, 31, 32, 33] {
        let mut link = Link::new(Layout::new(4, 33).unwrap());
        let data: Vec<_> = (0..len).map(|i| (i * 17 + 3) as u8).collect();
        link.send(7, 11, &data).unwrap();
        let mut out = vec![0; len];
        assert_eq!(
            link.recv(&mut out),
            Ok(Message {
                src: 7,
                tag: 11,
                len: len as u32
            })
        );
        assert_eq!(out, data);
    }
}

#[test]
fn full_and_empty_follow_depth() {
    let mut link = Link::new(Layout::new(2, 1).unwrap());
    assert_eq!(link.recv(&mut [0]), Err(RecvError::Empty));
    link.send(0, 0, &[1]).unwrap();
    link.send(0, 0, &[2]).unwrap();
    assert_eq!(link.send(0, 0, &[3]), Err(SendError::Full));
    let mut out = [0];
    link.recv(&mut out).unwrap();
    assert_eq!(out, [1]);
    link.send(0, 0, &[3]).unwrap();
    for expected in [2, 3] {
        link.recv(&mut out).unwrap();
        assert_eq!(out, [expected]);
    }
    assert_eq!(link.recv(&mut out), Err(RecvError::Empty));
}

#[test]
fn too_small_does_not_consume_or_touch_output() {
    let mut link = Link::new(Layout::new(2, 8).unwrap());
    link.send(3, 5, &[1, 2, 3]).unwrap();
    let mut short = [9, 9];
    assert_eq!(
        link.recv(&mut short),
        Err(RecvError::TooSmall { needed: 3 })
    );
    assert_eq!(short, [9, 9]);
    let mut out = [0; 3];
    assert_eq!(link.recv(&mut out).unwrap().tag, 5);
    assert_eq!(out, [1, 2, 3]);
}

#[test]
fn oversized_send_does_not_change_the_link() {
    let mut link = Link::new(Layout::new(2, 2).unwrap());
    assert_eq!(link.send(0, 0, &[1, 2, 3]), Err(SendError::TooLarge));
    assert_eq!(link.recv(&mut [0; 2]), Err(RecvError::Empty));
}

#[test]
fn sequence_wrap_reuses_the_right_slots() {
    let layout = Layout::new(4, 1).unwrap();
    let mut link = Link::new(layout);
    let start = u32::MAX - 2;
    link.send = start;
    link.recv = start;
    for i in 0..layout.depth() {
        let seq = start.wrapping_add(i);
        link.arena[layout.slot(seq)] = seq;
    }

    for value in 0..12u8 {
        link.send(0, 0, &[value]).unwrap();
        let mut out = [0];
        link.recv(&mut out).unwrap();
        assert_eq!(out, [value]);
    }
}
