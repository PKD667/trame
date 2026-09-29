//~ says: `#[parallel]` takes its context as a shared `&C`: every item reads it at once
struct W;
impl W {
    #[trame::parallel]
    fn add(&self, at: u8, cx: &mut u32) -> Result<(), ()> {
        let _ = (at, cx);
        Ok(())
    }
}
