//~ says: `#[parallel]` takes a plain `fn`, not `async fn`
#[trame::parallel]
async fn charge(at: usize, cx: &u64) -> Result<(), ()> {
    *cx += at as u64;
    Ok(())
}
