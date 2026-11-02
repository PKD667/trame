// spec: Requirement (All levels): no globals, at any level. A worker reads a static.
static COUNT: u32 = 7;

fn main() {
    trame::__check::worker(&|| {
        let _ = COUNT + 1; //~ ERROR: [static] worker
    });
}
