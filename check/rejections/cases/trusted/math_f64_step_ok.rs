//@check-pass
// spec: Section 7 (math::f64): the portable functions are accepted in a step as well as in an item; the step level
// admits more of core and alloc, never less. A step that integrates a field with sin, cos and sqrt needs no effect.
#[trame::message]
struct Job {
    id: u32,
}

#[derive(trame::Plain)]
struct Body {
    angle: f64,
    speed: f64,
    x: f64,
    y: f64,
}

#[trame::state(steps(step))]
struct S {
    body: Body,
}

#[trame::step(S, receives = Job)]
fn step(body: &mut Body) -> trame::Status {
    let (s, c) = trame::math::f64::sin_cos(body.angle);
    body.x += body.speed * c;
    body.y += body.speed * s;
    body.speed = trame::math::f64::sqrt(trame::math::f64::mul_add(body.x, body.x, body.y * body.y));
    trame::Status::Done
}

fn main() {}
