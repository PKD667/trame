//~ says: `#[parallel]` returns `Result<(), E>`
struct W;
impl W {
    #[trame::parallel]
    fn count(&self, at: usize, sum: &mut usize) {
        *sum += at;
    }
}
