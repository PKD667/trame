//~ says: `other: & mut u64` is a second `&mut`: only an `#[ordered]` slot is `&mut`
#[trame::parallel]
#[trame::ordered(key = at: usize)]
fn charge(at: usize, slot: &mut u64, other: &mut u64, cx: &()) -> Result<(), ()> {
    *slot += *other;
    Ok(())
}
