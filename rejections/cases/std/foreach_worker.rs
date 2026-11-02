// spec: Requirement (Worker): the closure passed to for_each is called by core, so it is checked at the
// worker's level, and its std call is refused.
fn main() {
    trame::__check::worker(&|| {
        [1u32, 2].iter().for_each(|_| std::println!("x")); //~ ERROR: [std] worker
    });
}
