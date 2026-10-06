// expect: recv(..) must be the only receiving arm in concurrent!
fn receives(cx: &mut trame::Context) {
    trame::concurrent!(cx;
        recv(..) => (),
        recv(trame::Tag::new(7)) => (),
    );
}
