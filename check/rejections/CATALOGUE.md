# Rejection suite: case catalogue

Every case under `cases/`, by directory. A case is accepted (`//@check-pass`) or refused. A refused case
lists the rules its annotations name. The `error` tag is a refusal by rustc or a macro, not by the driver.
Generated from the case tree when this file was written: a case not listed here is a documentation error.

## allocation (15)

Refused: `box_new_step` [allocation], `helper_alloc_from_item` [allocation], `string_push_step` [allocation], `vec_extend_step` [allocation], `vec_local_step` [allocation], `vec_macro_step` [allocation], `vec_new_len_item` [allocation], `vec_push_item` [allocation], `vec_push_step` [allocation], `vec_with_capacity_item` [allocation].

Accepted: `box_slice_step_ok`, `helper_pure_ok`, `string_read_step_ok`, `vec_index_assign_step_ok`, `vec_push_worker_ok`.

## allowlist (4)

Refused: `forget_item` [allow-list].

Accepted: `bool_then_step_ok`, `swap_item_ok`, `trait_default_user_item_ok`.

## checked (3)

Refused: `parallel_no_cfg` [error], `state_no_cfg` [error].

Accepted: `cfg_passed_ok`.

## counted-loop (11)

Refused: `loop_step` [counted-loop], `next_ignored_item` [counted-loop], `vec_iter_mutated_step` [error], `while_item` [counted-loop], `while_step` [counted-loop].

Accepted: `array_rev_step_ok`, `for_range_step_ok`, `item_loop_ok`, `rev_range_step_ok`, `rev_slice_step_ok`, `vec_iter_ok`.

## deny (8)

Refused: `atomic_worker` [deny], `cell_item` [deny], `cell_worker` [deny], `refcell_step` [deny], `repeat_step` [deny], `unbounded_range_step` [deny].

Accepted: `cell_worker_ok`, `unbounded_range_step_ok`.

## drop (8)

Refused: `box_drop_std_step` [allocation,std], `box_drop_unsafe_step` [allocation,unsafe], `box_u32_drop_step` [allocation], `take_assign_step` [allocation], `unwind_vec_normal_step` [allocation], `unwind_vec_step_ok` [allocation], `user_drop_unsafe_in_vec_step` [allocation,unsafe].

Accepted: `take_put_back_step_ok`.

## effects (11)

Refused: `now_item` [effects], `parallel_item` [effects], `recv_item` [effects], `send_item` [effects], `strictest_level_item` [allocation].

Accepted: `now_step_ok`, `parallel_step_ok`, `send_item_worker_ok`, `send_step_ok`, `step_mixed_ok`, `strictest_level_worker_ok`.

## ffi (4)

Refused: `extern_item` [ffi,unsafe], `extern_step` [ffi,unsafe], `extern_worker` [ffi,unsafe].

Accepted: `extern_item_ok`.

## flow (4)

Refused: `drain_vec_push_step` [allocation].

Accepted: `bounded_step_ok`, `drain_step_ok`, `fuel_step_ok`.

## float (8)

Refused: `f64_max_item` [allow-list], `f64_mul_add_item` [std], `f64_sin_item` [std], `f64_sqrt_item` [std].

Accepted: `float_bits_item_ok`, `float_ieee_item_ok`, `float_signum_clamp_item_ok`, `math_sqrt_item_ok`.

## indirect (6)

Refused: `dyn_fn_worker` [indirect], `dyn_trait_step` [indirect], `fn_pointer_worker` [indirect], `indirect_item` [indirect].

Accepted: `dyn_fn_worker_ok`, `fn_pointer_worker_ok`.

## message (13)

Refused: `derive_clone` [error], `fn_pointer` [error], `generic` [error], `padding` [error], `raw_pointer` [error], `reference_field` [error], `reference_field_nested` [error], `repr_packed` [error], `zeroed_user_impl` [allow-list,unsafe].

Accepted: `array_field_ok`, `padding_ok`, `plain_ok`, `zeroed_ok`.

## recursion (10)

Refused: `clone_recursion_step` [allocation,recursion], `drop_recursion_item` [recursion], `recursion_item` [recursion], `recursion_step` [recursion], `recursion_step_mutual` [recursion], `recursion_via_closure_item` [recursion].

Accepted: `diamond_item_ok`, `recursion_false_positive_ok`, `recursion_item_worker_ok`, `recursion_worker_ok`.

## state (4)

Refused: `param_not_a_field` [error], `param_wrong_type` [error], `unsafe_fn_step` [error].

Accepted: `array_receives_ok`.

## plain (24)

What a state and its pool hold is the type `trame::Plain` (CONTRACT.md, M2, M3): rustc refuses, with E0277 "cannot be held by a trame state, pool or error", at the field.

Refused: `arc`, `atomic`, `cell_in_pool`, `dyn_box`, `enum_variant`, `file`, `fn_pointer`, `instant`, `mutex`, `nested_cell`, `no_derive`, `oncelock`, `pool_not_a_struct`, `raw_pointer_wrapper`, `rc_wrapper`, `reference_in_vec`, `reference_static`, `rwlock`, `vec_of_nested`, `weak`.

Accepted: `nested_ok`, `plain_ok`, `pool_ok`, `worker_id_ok`.

## static (7)

Refused: `mut_step` [static,unsafe], `read_item` [static], `read_step` [static], `read_worker` [static], `user_drop_static_step` [static,unsafe].

Accepted: `read_step_ok`, `read_worker_ok`.

## std (18)

Refused: `abort_item` [std], `drop_glue_item` [std], `foreach_step` [std], `foreach_worker` [std], `println_item` [std], `println_step` [std], `println_worker` [std], `thread_spawn_item` [allocation,std], `thread_spawn_step` [std], `thread_spawn_worker` [std].

Accepted: `foreach_worker_ok`, `option_expect_step_ok`, `panic_const_step_ok`, `panic_fmt_a_generic_fn_step_ok`, `panic_fmt_b_method_step_ok`, `panic_fmt_c_u64_after_send_step_ok`, `panic_fmt_d_nerve_shape_step_ok`, `panic_fmt_step_ok`.

## trusted (4)

Accepted: `math_exp_item_ok`, `math_f64_every_item_ok`, `math_f64_item_ok`, `math_f64_step_ok`.

## unsafe (18)

Refused: `block_item` [unsafe], `block_step` [unsafe], `block_worker` [unsafe], `cmp_max_user_ord_item` [unsafe], `direct_dependency` [allow-list,unsafe], `fn_item_into_core_item` [unsafe], `fn_item_output_max_item` [unsafe], `generic_closure_unsafe_step` [unsafe], `impl_trait_return_step` [unsafe], `ptr_read_method_item` [deny,unsafe], `sort_user_ord_item` [unsafe], `transmute_item` [unsafe], `user_clone_unsafe_step` [unsafe], `user_eq_unsafe_item` [unsafe], `user_iter_unsafe_item` [unbounded,unsafe].

Accepted: `block_item_ok`, `box_deref_step_ok`, `box_field_item_ok`.
