fn requires_sync<T: Sync>() {}
pub fn obligation() {
    requires_sync::<trame::Shared>(); // P1-OBLIGATION: E0277 Sync
}
