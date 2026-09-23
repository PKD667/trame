//~ says: `#[parallel]` takes its context as `&mut C`: each call owns it
struct W;
impl W {
    #[trame::parallel]
    fn add(&self, at: u8, cx: &u32) -> Result<(), ()> {
        let _ = (at, cx);
        Ok(())
    }
}
