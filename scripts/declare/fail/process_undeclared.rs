// expect: no method named `__declared`
struct Idle;
impl Idle {
    fn step(&mut self) -> Result<trame::Step, ()> {
        Ok(trame::Step::Done)
    }
}
pub fn run() -> Result<(), ()> {
    trame::concurrent!(Idle)
}
