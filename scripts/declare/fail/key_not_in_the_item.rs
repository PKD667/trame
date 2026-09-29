//~ says: the key is a place in the item, like `hit.field: K`; `self . cell` is not
#[derive(Clone, Copy)]
struct Hit {
    cell: usize,
}
struct W {
    cell: usize,
}
impl W {
    #[trame::parallel]
    #[trame::ordered(key = self.cell: usize)]
    fn charge(&self, hit: Hit, slot: &mut u64, cx: &()) -> Result<(), ()> {
        *slot += hit.cell as u64;
        Ok(())
    }
}
