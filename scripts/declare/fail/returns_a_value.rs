//~ says: `#[parallel]` returns `Result<(), E>` or nothing
struct W;
impl W {
    #[trame::parallel]
    fn count(&self, at: usize, sum: &std::sync::atomic::AtomicUsize) -> usize {
        at + sum.load(std::sync::atomic::Ordering::Relaxed)
    }
}
