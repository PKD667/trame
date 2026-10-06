// expect: receive tag has multiple arm owners
fn receives(cx: &mut trame::Context) {
    trame::concurrent!(cx;
        recv(trame::Tag::new(7)) => (),
        recv(trame::Tag::new(7)) => (),
    );
}
