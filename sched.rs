//! Scheduling core of the sequential backend: spec.md section 5.3, rules Start, End, Done, Fail.
//! Plain data and pure functions. No clocks, threads or I/O. The caller computes `Now` from the
//! world, and applies the effect that Start prescribes: set `running` and add `cells` to `held`.

/// St(p): the status a process last returned. `Wait` carries its conditions Ω. `cells` is
/// `Wait::cells()`, the empty Ω: it adds no condition, so such a wait is woken only by D(p).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum St {
    Fresh,
    Progress,
    Done,
    Wait { frame: bool, room: bool, until: bool, cells: bool },
    Err,
}

/// One process of a `concurrent!`. Its index in the slice is its priority π(p).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    /// L(p): the cells of p, as a bitmask over the concurrent!'s cells. At most 64 cells.
    pub cells: u64,
    /// St(p).
    pub st: St,
    /// D(p): another process freed a cell of p since p last ended.
    pub d: bool,
    /// Π(p) is not ⊥: a step of p is running.
    pub running: bool,
}

/// Per-process bitmasks (bit p): which wait conditions hold now. Computed by the caller from the
/// world. Bit p is only read for a process p whose wait has that condition, so at most 64
/// processes can wait on a condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub frame: u64,
    pub room: u64,
    pub until: u64,
}

fn bit(mask: u64, i: usize) -> bool {
    assert!(i < 64, "a wait condition can only be read for process index < 64");
    mask >> i & 1 == 1
}

/// Definition (Enabled): St(p) is fresh or progress, or St(p) is wait(Ω) and some condition in Ω
/// holds or D(p) is true.
pub fn enabled(p: &Proc, i: usize, now: &Now) -> bool {
    match p.st {
        St::Fresh | St::Progress => true,
        St::Done | St::Err => false,
        St::Wait { frame, room, until, cells: _ } => {
            p.d || (frame && bit(now.frame, i)) || (room && bit(now.room, i)) || (until && bit(now.until, i))
        }
    }
}

/// Start, for one process: not running, enabled, and its cells free.
fn ready(ps: &[Proc], i: usize, held: u64, now: &Now) -> bool {
    let p = &ps[i];
    !p.running && enabled(p, i, now) && p.cells & held == 0
}

/// Start: the index of the process that may begin a step now, or None. No process may start if
/// some process is err. Otherwise the lowest-index ready process starts: any lower-index ready
/// process would block it. Pure: the caller applies the effect.
pub fn start(ps: &[Proc], held: u64, now: &Now) -> Option<usize> {
    if ps.iter().any(|p| p.st == St::Err) {
        return None;
    }
    (0..ps.len()).find(|&i| ready(ps, i, held, now))
}

/// End: the step of process i returned s. Releases its cells, records s, and marks every other
/// process that shares a cell with i, whatever s is. A waiter is woken even when s is a wait,
/// because the step may have changed the cell before it waited.
pub fn end(ps: &mut [Proc], i: usize, s: St, held: &mut u64) {
    let cells = ps[i].cells;
    *held &= !cells;
    for (q, other) in ps.iter_mut().enumerate() {
        if q != i && other.cells & cells != 0 {
            other.d = true;
        }
    }
    let p = &mut ps[i];
    p.running = false;
    p.st = s;
    p.d = false;
}

/// Done: every process is done, answer Ok. Fail: some process is err and none is running, answer
/// Err with the lowest such index. Otherwise the concurrent! has not answered yet: None.
pub fn answer(ps: &[Proc]) -> Option<Result<(), usize>> {
    if ps.iter().all(|p| p.st == St::Done) {
        return Some(Ok(()));
    }
    if ps.iter().any(|p| p.running) {
        return None;
    }
    ps.iter().position(|p| p.st == St::Err).map(Err)
}

#[cfg(test)]
mod support {
    use super::*;

    pub const NOW0: Now = Now { frame: 0, room: 0, until: 0 };
    pub const FRESH: Proc = Proc { cells: 0, st: St::Fresh, d: false, running: false };
    pub const WAIT_ROOM: St = St::Wait { frame: false, room: true, until: false, cells: false };

    /// A fresh process over the given cells.
    pub fn at(cells: u64) -> Proc {
        Proc { cells, ..FRESH }
    }
}

#[cfg(test)]
mod start_tests {
    extern crate std;
    use super::support::*;
    use super::*;

    #[test]
    fn fresh_free_process_starts() {
        assert_eq!(start(&[at(1)], 0, &NOW0), Some(0));
    }

    #[test]
    fn premise_not_running() {
        let ps = [Proc { running: true, ..at(1) }];
        assert_eq!(start(&ps, 0, &NOW0), None);
    }

    #[test]
    fn premise_enabled_done_is_not_enabled() {
        let ps = [Proc { st: St::Done, ..at(1) }];
        assert_eq!(start(&ps, 0, &NOW0), None);
    }

    #[test]
    fn premise_enabled_wait_needs_condition_or_d() {
        let waiting = [Proc { st: WAIT_ROOM, ..at(1) }];
        assert_eq!(start(&waiting, 0, &NOW0), None, "no condition holds");
        let holds = Now { room: 1, ..NOW0 };
        assert_eq!(start(&waiting, 0, &holds), Some(0), "room(0) holds");
        let woken = [Proc { st: WAIT_ROOM, d: true, ..at(1) }];
        assert_eq!(start(&woken, 0, &NOW0), Some(0), "D(p) enables a wait");
    }

    #[test]
    fn premise_cells_free() {
        let ps = [at(0b10)];
        assert_eq!(start(&ps, 0b10, &NOW0), None, "its cell is held");
        assert_eq!(start(&ps, 0b01, &NOW0), Some(0), "held cell is not its cell");
    }

    #[test]
    fn premise_no_err_anywhere() {
        let ps = [at(1), Proc { st: St::Err, ..at(2) }];
        assert_eq!(start(&ps, 0, &NOW0), None, "an err blocks a free process");
        let only_err = [Proc { st: St::Err, ..at(1) }];
        assert_eq!(start(&only_err, 0, &NOW0), None);
    }

    #[test]
    fn premise_no_lower_index_ready_process() {
        // Process 0 is ready, so process 1 must not start: 0 has priority.
        let ps = [at(0b01), at(0b10)];
        assert_eq!(start(&ps, 0, &NOW0), Some(0));
        // Process 0 is not ready (its cell is held), so it does not block 1.
        assert_eq!(start(&ps, 0b01, &NOW0), Some(1));
        // Process 0 is running, so it does not block 1 either.
        let running = [Proc { running: true, ..at(0b01) }, at(0b10)];
        assert_eq!(start(&running, 0b01, &NOW0), Some(1));
        // Process 0 is not enabled (done), so it does not block 1.
        let done = [Proc { st: St::Done, ..at(0b01) }, at(0b10)];
        assert_eq!(start(&done, 0, &NOW0), Some(1));
    }

    #[test]
    fn running_process_without_cells_does_not_block() {
        // Choice recorded in the report: the spec writes "H(L(q)) = bot", which a process with
        // no cells satisfies even while it runs. The "not running" clause of the task excludes it.
        let ps = [Proc { running: true, ..FRESH }, at(0b10)];
        assert_eq!(start(&ps, 0, &NOW0), Some(1));
    }
}

#[cfg(test)]
mod end_answer_tests {
    extern crate std;
    use super::support::*;
    use super::*;

    #[test]
    fn end_releases_cells_and_records_status() {
        let mut ps = [Proc { running: true, st: St::Progress, d: true, ..at(0b01) }, at(0b10)];
        let mut held = 0b01;
        end(&mut ps, 0, St::Done, &mut held);
        assert_eq!(held, 0);
        assert!(!ps[0].running);
        assert_eq!(ps[0].st, St::Done);
        assert!(!ps[0].d, "D(p) is reset by End");
    }

    #[test]
    fn end_wakes_cell_sharing_waiter_even_when_status_is_a_wait() {
        let mut ps = [
            Proc { running: true, st: St::Progress, ..at(0b01) },
            Proc { st: WAIT_ROOM, ..at(0b01) },
            Proc { st: WAIT_ROOM, ..at(0b10) },
        ];
        let mut held = 0b01;
        end(&mut ps, 0, WAIT_ROOM, &mut held);
        assert_eq!(ps[0].st, WAIT_ROOM, "the wait is recorded");
        assert!(ps[1].d, "a cell-sharing waiter is woken");
        assert!(!ps[2].d, "a waiter on other cells is not woken");
        assert!(!enabled(&ps[2], 2, &NOW0));
        assert!(enabled(&ps[1], 1, &NOW0), "D(q) makes the sharing waiter enabled");
        assert_eq!(start(&ps, held, &NOW0), Some(1));
    }

    #[test]
    fn answer_done_when_all_done() {
        let ps = [Proc { st: St::Done, ..at(1) }, Proc { st: St::Done, ..at(2) }];
        assert_eq!(answer(&ps), Some(Ok(())));
        assert_eq!(answer(&[]), Some(Ok(())));
    }

    #[test]
    fn answer_fail_with_lowest_err_when_none_running() {
        let ps = [Proc { st: St::Done, ..at(1) }, Proc { st: St::Err, ..at(2) }, Proc { st: St::Err, ..at(4) }];
        assert_eq!(answer(&ps), Some(Err(1)));
    }

    #[test]
    fn answer_waits_while_something_runs_or_is_pending() {
        let running = [Proc { st: St::Err, ..at(1) }, Proc { running: true, ..at(2) }];
        assert_eq!(answer(&running), None, "Fail needs no running process");
        let pending = [Proc { st: St::Done, ..at(1) }, FRESH];
        assert_eq!(answer(&pending), None, "a fresh process is neither done nor failed");
    }
}

#[cfg(test)]
mod property_tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// xorshift64, fixed seed: no crates, same states on every run.
    struct Rng(u64);

    impl Rng {
        fn word(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: u64) -> u64 {
            self.word() % n
        }
        fn flag(&mut self) -> bool {
            self.word() & 1 == 1
        }
    }

    /// Literal transcription of the Start premises, with the quantifiers written out. Process p
    /// may start when every premise holds. The rule implies at most one such p.
    fn spec_start(ps: &[Proc], held: u64, now: &Now) -> Option<usize> {
        let n = ps.len();
        let free = |q: usize| ps[q].cells & held == 0; // H(L(q)) = bot
        let ok: Vec<usize> = (0..n)
            .filter(|&p| {
                !ps[p].running                                   // Pi(p) = bot
                    && enabled(&ps[p], p, now)                   // p enabled
                    && free(p)                                   // H(L(p)) = bot
                    && (0..n).all(|q| ps[q].st != St::Err)       // for all q: St(q) != err
                    && !(0..n).any(|q| {                         // not exists q:
                        enabled(&ps[q], q, now)                  //   q enabled
                            && q < p                             //   and lower priority
                            && !ps[q].running                    //   (not running, see test note)
                            && free(q)                           //   and H(L(q)) = bot
                    })
            })
            .collect();
        assert!(ok.len() <= 1, "premises admit two processes: {ok:?}");
        ok.first().copied()
    }

    fn random_state(rng: &mut Rng) -> (Vec<Proc>, u64, Now) {
        let n = rng.below(7) as usize;
        let ps = (0..n)
            .map(|_| {
                let cells = if rng.below(8) == 0 { 0 } else { rng.word() & rng.word() & 0xFF };
                let st = match rng.below(7) {
                    0 => St::Fresh,
                    1 => St::Progress,
                    2 => St::Done,
                    3 => St::Err,
                    _ => St::Wait {
                        frame: rng.flag(),
                        room: rng.flag(),
                        until: rng.flag(),
                        cells: rng.flag(),
                    },
                };
                Proc { cells, st, d: rng.flag(), running: rng.below(4) == 0 }
            })
            .collect();
        let held = rng.word() & rng.word() & 0xFF;
        let now = Now { frame: rng.word() & rng.word(), room: rng.word() & rng.word(), until: rng.word() & rng.word() };
        (ps, held, now)
    }

    #[test]
    fn start_matches_spec_rule_on_random_states() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let (mut some, mut none, mut later) = (0u32, 0u32, 0u32);
        for case in 0..100_000 {
            let (ps, held, now) = random_state(&mut rng);
            let got = start(&ps, held, &now);
            let want = spec_start(&ps, held, &now);
            assert_eq!(got, want, "case {case}: held={held:#x} now={now:?} ps={ps:?}");
            match got {
                Some(0) => some += 1,
                Some(_) => later += 1,
                None => none += 1,
            }
        }
        // The test must not be vacuous: every outcome must occur many times.
        assert!(some > 1_000 && later > 1_000 && none > 1_000, "some={some} later={later} none={none}");
    }

    // Bounded model check of the Rust functions against the spec rule, not a proof for all sizes.
    #[test]
    fn start_and_end_exhaustive_up_to_bounds() {
        // St values: fresh, progress, done, err, and the 16 flag combinations of wait(Omega).
        fn st_of(k: usize) -> St {
            match k {
                0 => St::Fresh,
                1 => St::Progress,
                2 => St::Done,
                3 => St::Err,
                _ => {
                    let w = k - 4;
                    St::Wait { frame: w & 8 != 0, room: w & 4 != 0, until: w & 2 != 0, cells: w & 1 != 0 }
                }
            }
        }
        const IDLE: Proc = Proc { cells: 0, st: St::Fresh, d: false, running: false };
        let all_st: Vec<St> = (0..20).map(st_of).collect();

        // (processes, cells) pairs, each enumerated completely.
        for &(n, c) in &[(0usize, 3u32), (1, 3), (2, 3)] {
            let m = 1usize << c; // cell masks 0..m
            let per = 20 * 2 * 2 * m; // per-process codes: St, d, running, cells
            let nows = 1usize << (3 * n); // frame/room/until bit for each process index < n
            let (mut states, mut ends) = (0u64, 0u64);
            for x in 0..per.pow(n as u32) {
                let mut ps = [IDLE; 3];
                let mut rest = x;
                for slot in ps.iter_mut().take(n) {
                    let k = rest % per;
                    rest /= per;
                    let q = k / m; // q = st + 20 * d + 40 * running
                    *slot = Proc {
                        cells: (k % m) as u64,
                        st: st_of(q % 20),
                        d: (q / 20) % 2 == 1,
                        running: q / 40 == 1,
                    };
                }
                let procs = &ps[..n];
                // Consistent states only: running processes hold disjoint cells.
                let mut held = 0u64;
                let mut consistent = true;
                for p in procs.iter().filter(|p| p.running) {
                    consistent &= p.cells & held == 0;
                    held |= p.cells;
                }
                if !consistent {
                    continue;
                }
                for u in 0..nows {
                    let mut now = Now { frame: 0, room: 0, until: 0 };
                    for i in 0..n {
                        let b = 3 * i;
                        now.frame |= ((u >> b) as u64 & 1) << i;
                        now.room |= ((u >> (b + 1)) as u64 & 1) << i;
                        now.until |= ((u >> (b + 2)) as u64 & 1) << i;
                    }
                    states += 1;

                    // (1), (2): start returns the unique process the literal premises pick, or None.
                    // spec_start asserts that at most one process satisfies them.
                    let got = start(procs, held, &now);
                    let want = spec_start(procs, held, &now);
                    assert_eq!(got, want, "n={n} c={c} ps={procs:?} held={held:#x} now={now:?}");

                    // (3): End for each running i and each returned status s.
                    for i in (0..n).filter(|&i| procs[i].running) {
                        let cells_i = procs[i].cells;
                        for s in &all_st {
                            let mut after = ps;
                            let mut h = held;
                            end(&mut after[..n], i, *s, &mut h);
                            ends += 1;
                            let ctx = || std::format!("n={n} c={c} ps={procs:?} held={held:#x} i={i} s={s:?}");
                            assert_eq!(h & cells_i, 0, "cells of i not free: {}", ctx());
                            assert!(!after[i].d, "d[i] not false: {}", ctx());
                            assert!(!after[i].running && after[i].st == *s, "record: {}", ctx());
                            for j in (0..n).filter(|&j| j != i) {
                                if procs[j].cells & cells_i != 0 {
                                    assert!(after[j].d, "d[q] not true, q={j}: {}", ctx());
                                }
                            }
                        }
                    }
                }
            }
            std::println!("n={n} c={c}: {states} consistent states, {ends} end checks");
            assert!(states > 0);
        }
    }
}
