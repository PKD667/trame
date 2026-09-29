fn requires_sync<T: Sync>() {}
pub fn obligation() {
    requires_sync::<trame::Io<'_>>(); // P1-OBLIGATION: E0277 Sync
}
