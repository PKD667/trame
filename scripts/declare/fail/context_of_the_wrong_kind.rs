//~ says: `#[concurrent]` takes its context as `&C`: every unit shares it; `cx: & mut u32` is not
struct W;
impl W {
    #[trame::concurrent]
    fn role(&self, role: u8, cx: &mut u32) -> Result<(), ()> {
        *cx += role as u32;
        Ok(())
    }
}
