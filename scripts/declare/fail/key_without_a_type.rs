//~ says: write the key's type: `key = hit . cell: K`
#[derive(Clone, Copy)]
struct Hit {
    cell: usize,
}
#[trame::parallel]
#[trame::ordered(key = hit.cell)]
fn charge(hit: Hit, slot: &mut u64, cx: &()) -> Result<(), ()> {
    *slot += hit.cell as u64;
    Ok(())
}
