use trame::leader::Published;
pub fn obligation(value: Published) {
    let _ = trame::bytes(&value); // P1-OBLIGATION: E0308 Shared
}
