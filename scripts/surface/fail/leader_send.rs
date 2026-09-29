fn requires_send<T: Send>() {}
pub fn obligation() {
    requires_send::<trame::leader::Leader>(); // P1-OBLIGATION: E0277 Send
}
