//@check-pass
// spec: Requirement (All levels): no globals. The pair of read_worker.rs and static/read_step.rs: a const, not a
// static, is a value and is accepted in a worker.
const COUNT: u32 = 7;

fn main() {
    trame::__check::worker(&|| {
        let _ = COUNT + 1;
    });
}
