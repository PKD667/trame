fn requires_send<T: Send>() {}
pub fn obligation(value: trame::Shared) {
    requires_send::<trame::Shared>(); // P1-OBLIGATION: E0277 Send
    let _ = value;
}
