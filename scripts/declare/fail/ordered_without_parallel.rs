//~ says: `#[ordered]` orders a `#[parallel]` function and is written below it
#[trame::ordered(key = at: usize)]
fn charge(at: usize, slot: &mut u64, cx: &mut ()) -> Result<(), ()> {
    *slot += at as u64;
    Ok(())
}
