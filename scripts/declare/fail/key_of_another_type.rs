//~ says: mismatched types
#[derive(Clone, Copy)]
struct Hit {
    cell: u8,
}
#[trame::parallel]
#[trame::ordered(key = hit.cell: usize)]
fn charge(hit: Hit, slot: &mut u64, cx: &()) -> Result<(), ()> {
    *slot += hit.cell as u64;
    Ok(())
}
