use trame::Context;
fn requires_sync<T: Sync>() {}
pub fn obligation() {
    requires_sync::<Context>(); // P1-OBLIGATION: E0277 Sync
}
