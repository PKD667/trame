//~ says: `other: & mut u64` is a second `&mut`: only a `#[parallel]` context and an `#[ordered]` slot are `&mut`
#[trame::parallel]
#[trame::ordered(key = at: usize)]
fn charge(at: usize, slot: &mut u64, other: &mut u64, cx: &mut ()) -> Result<(), ()> {
    *slot += *other;
    Ok(())
}
