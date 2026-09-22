// The requirements a program states, checked at build time.
//
// `require!` is the only thing that turns a program's needs into a refusal, so a requirement the
// selected backend does declare must compile — otherwise the refusal is unusable and a caller
// cannot say what it needs. The refusing direction is exercised by `scripts/declare/`'s fixtures
// for the lowering; what can be checked here is that the accepting direction resolves at all and
// that the names a caller writes are the names the table has.

// The device declares these, so this compiles. Written at item position, not inside a test, so
// that the check is the declaration itself: a `const` in a function body is only evaluated when
// the body is code generated, and `cargo check` would pass a requirement `cargo build` refuses.
//
// There is no `require!(send)` and no `require!(turn)` any more, and their absence is the point:
// a name in the portable surface is something every backend answers, because every backend exports
// the same surface. A requirement that asked whether an exported name exists could only ever be
// answered `true`, and one that could be answered `false` was a backend that exported a name it did
// not answer.
crate::require!(reliable_lanes);
crate::require!(bounded_wait);
crate::require!(atomic_scope(domain));
// The device observes a full lane and a short buffer as refusals, so a caller that needs flow
// control gets it here. The refusing direction is `scripts/declare/fail/require_backpressure.rs`.
crate::require!(backpressure);

#[test]
fn the_names_a_program_may_require_are_the_names_the_table_has() {
    // Every requirement above names something the table declares. If one of them were misspelled
    // the macro would have failed to expand, which is the point of a closed set; this test is what
    // fails if the *check* was written against the wrong field.
    let declared = crate::nv::DECLARATIONS;
    assert!(declared.atomic_scopes.domain);
    assert_eq!(declared.lane_reliability, crate::LANE_RELIABLE);
}
