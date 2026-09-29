use trame::leader::Published;
fn requires_sync<T: Sync>() {}
pub fn obligation() {
    requires_sync::<Published>(); // P1-OBLIGATION: E0277 Sync
}
