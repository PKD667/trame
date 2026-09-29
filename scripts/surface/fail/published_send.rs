use trame::leader::Published;
fn requires_send<T: Send>() {}
pub fn obligation() {
    requires_send::<Published>(); // P1-OBLIGATION: E0277 Send
}
