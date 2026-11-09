// spec: Requirement (Worker): a worker reaches the outside world only through effects, so it never calls the OS.
fn main() {
    trame::__check::worker(&|| {
        std::println!("hello"); //~ ERROR: [std] worker
    });
}
