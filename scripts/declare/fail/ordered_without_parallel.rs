//~ says: `#[ordered]` orders a `#[parallel]` function and is written below it
#[trame::ordered(key = at: usize)]
fn charge(at: usize, slot: &mut u64, cx: &()) -> Result<(), ()> {
    *slot += at as u64;
    Ok(())
}
