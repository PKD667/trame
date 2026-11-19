//@error-in-other-file: [allocation] step
// spec: Requirement (All levels): drop glue counts as a call. `self.w = Some(w)` drops the old value first; it is None
// here, but the checker judges the glue of Option<W>, which names Vec::drop, not the value. The pair of
// take_put_back_step_ok.rs, which says `trame::put_back` for the same act.
#[derive(trame::Plain)]
struct W {
    v: Vec<u32>,
}

#[derive(trame::Plain)]
struct Delivery {
    w: Option<W>,
    n: u32,
}

impl Delivery {
    fn lend(&mut self, f: impl FnOnce(&mut Self, &mut W)) {
        let Some(mut w) = self.w.take() else { return };
        f(self, &mut w);
        self.w = Some(w);
    }
}

#[trame::state(steps(step))]
struct S {
    d: Delivery,
}

#[trame::step(S)]
fn step(d: &mut Delivery) -> trame::Status {
    d.lend(|d, w| d.n += w.v.len() as u32);
    trame::Status::Progress
}

fn main() {}
