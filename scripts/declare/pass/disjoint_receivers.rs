use core::convert::Infallible;

const LEFT: trame::Tag = trame::Tag::new(7);
const RIGHT: trame::Tag = trame::Tag::new(8);

#[trame::process]
struct Inbox;

impl Inbox {
    fn step(&mut self, _: &mut trame::Io<'_>) -> Result<trame::Step, Infallible> {
        Ok(trame::Step::Done)
    }
}

fn disjoint(cx: &mut trame::Context) -> Result<(), Infallible> {
    let (mut left, mut right) = (Inbox, Inbox);
    trame::concurrent!(cx;
        recv(self::LEFT, self::LEFT) => &mut left,
        recv(self::RIGHT) => &mut right,
    )
}
