// Round trips and the parser's panics for the relay records (wire.rs).

extern crate std;

use super::*;
use std::vec;

fn every_kind() -> Vec<Record<'static>> {
    vec![
        Record::Frames { route: 3, index: 1 << 40, bytes: b"frame" },
        Record::Frames { route: 0, index: 0, bytes: b"" },
        Record::Credit { route: 7, c: u64::MAX },
        Record::Arrived { generation: 9 },
        Record::Release { generation: 4, no: 2, bytes: b"publication" },
        Record::Result { worker: 5, outcome: 3 },
        Record::Exited { worker: 6, arrive: 12 },
        Record::Abort { worker: 2, bytes: b"worker 2 fails" },
        Record::Abort { worker: 0, bytes: b"" },
        Record::End,
    ]
}

#[test]
fn every_kind_round_trips() {
    for r in every_kind() {
        let mut buf = Vec::new();
        put(&mut buf, &r);
        let (back, used) = next(&buf).expect("one record");
        assert_eq!(back, r);
        assert_eq!(used, buf.len());
    }
}

#[test]
fn a_message_of_records_parses_in_order() {
    let mut buf = Vec::new();
    for r in every_kind() {
        put(&mut buf, &r);
    }
    let mut at = 0;
    let mut seen = Vec::new();
    while let Some((r, used)) = next(&buf[at..]) {
        seen.push(r);
        at += used;
    }
    assert_eq!(at, buf.len());
    assert_eq!(seen, every_kind());
}

#[test]
#[should_panic(expected = "truncated")]
fn a_truncated_record_panics() {
    let mut buf = Vec::new();
    put(&mut buf, &Record::Credit { route: 1, c: 2 });
    next(&buf[..buf.len() - 1]);
}

#[test]
#[should_panic(expected = "unknown kind")]
fn an_unknown_kind_panics() {
    next(&[42]);
}
