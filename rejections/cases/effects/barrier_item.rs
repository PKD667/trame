// spec: Requirement (Item), Definition (Effect sets): an item performs no effect (Sigma_I is empty); barrier is a worker effect.
// Refused [effects] item: the call to trame::barrier is an effect that no item may perform.
// Refused [allocation] item: barrier returns an Arc<[u8]> that `let _ =` drops at once. Arc::drop calls alloc, and an item neither
// allocates nor calls alloc (Requirement (Item)). A_W.txt lists Arc::drop at worker level only.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn wait_all(x: &Work, slot: &mut u32, _cx: &()) {
    let _ = trame::barrier();
    //~^ ERROR: [effects] item
    //~| ERROR: [allocation] item
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(wait_all, &(), &items, &mut slots);
}
