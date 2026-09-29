fn requires_sync<T: Sync>() {}
pub fn obligation() {
    requires_sync::<trame::leader::Leader>(); // P1-OBLIGATION: E0277 Sync
}
