//~ says: `slot: & mut u64` is keyed state, which only an `#[ordered]` `#[parallel]` function takes
#[trame::parallel]
fn charge(at: usize, slot: &mut u64, cx: &()) -> Result<(), ()> {
    *slot += at as u64;
    Ok(())
}
