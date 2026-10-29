# Rejection suite: case catalogue

Every case under `cases/`, by directory. A case is accepted (`//@check-pass`) or refused. A refused case
lists the rules its annotations name. The `error` tag is a refusal by rustc or a macro, not by the driver.
Generated from the case tree when this file was written: a case not listed here is a documentation error.

## allocation (8)

Refused: `helper_alloc_from_item` [allocation], `vec_new_len_item` [allocation], `vec_push_item` [allocation], `vec_with_capacity_item` [allocation].

Accepted: `helper_pure_ok`, `vec_macro_step_ok`, `vec_push_step_ok`, `vec_push_worker_ok`.

## allowlist (4)

Refused: `forget_item` [allow-list].

Accepted: `bool_then_step_ok`, `swap_item_ok`, `trait_default_user_item_ok`.

## checked (3)

Refused: `concurrent_no_cfg` [error], `parallel_no_cfg` [error].

Accepted: `cfg_passed_ok`.

## counted-loop (10)

Refused: `loop_step` [counted-loop], `vec_iter_mutated_step` [error], `while_item` [counted-loop], `while_step` [counted-loop].

Accepted: `array_rev_step_ok`, `for_range_step_ok`, `item_loop_ok`, `rev_range_step_ok`, `rev_slice_step_ok`, `vec_iter_ok`.

## deny (8)

Refused: `atomic_worker` [deny], `cell_item` [deny], `cell_worker` [deny], `refcell_step` [deny], `repeat_step` [deny], `unbounded_range_step` [deny].

Accepted: `cell_worker_ok`, `unbounded_range_step_ok`.

## drop (4)

Refused: `box_drop_std_step` [std], `box_drop_unsafe_step` [unsafe], `user_drop_unsafe_in_vec_step` [unsafe].

Accepted: `box_u32_drop_step_ok`.

## effects (17)

Refused: `barrier_item` [allocation,effects], `barrier_step` [allow-list,effects], `concurrent_item` [effects], `concurrent_step` [effects], `now_item` [effects], `parallel_item` [effects], `recv_item` [effects], `send_item` [effects], `strictest_level_item` [allocation,effects].

Accepted: `barrier_worker_ok`, `concurrent_worker_ok`, `now_step_ok`, `parallel_step_ok`, `send_item_worker_ok`, `send_step_ok`, `step_mixed_ok`, `strictest_level_worker_ok`.

## ffi (4)

Refused: `extern_item` [ffi,unsafe], `extern_step` [ffi,unsafe], `extern_worker` [ffi,unsafe].

Accepted: `extern_item_ok`.

## float (5)

Refused: `f64_max_item` [allow-list], `f64_sqrt_item` [std].

Accepted: `float_bits_item_ok`, `float_ieee_item_ok`, `float_signum_clamp_item_ok`, `math_sqrt_item_ok`.

## indirect (6)

Refused: `dyn_fn_worker` [indirect], `dyn_trait_step` [indirect], `fn_pointer_worker` [indirect], `indirect_item` [indirect].

Accepted: `dyn_fn_worker_ok`, `fn_pointer_worker_ok`.

## message (13)

Refused: `derive_clone` [error], `fn_pointer` [error], `generic` [error], `padding` [error], `raw_pointer` [error], `reference_field` [error], `reference_field_nested` [error], `repr_packed` [error], `zeroed_user_impl` [allow-list,unsafe].

Accepted: `array_field_ok`, `padding_ok`, `plain_ok`, `zeroed_ok`.

## process (4)

Refused: `cell_primitive_u32` [error], `cell_vec_u32` [error].

Accepted: `array_receives_ok`, `cell_newtype_ok`.

## recursion (10)

Refused: `clone_recursion_step` [recursion], `drop_recursion_item` [recursion], `recursion_item` [recursion], `recursion_step` [recursion], `recursion_step_mutual` [recursion], `recursion_via_closure_item` [recursion].

Accepted: `diamond_item_ok`, `recursion_false_positive_ok`, `recursion_item_worker_ok`, `recursion_worker_ok`.

## static (7)

Refused: `mut_step` [static,unsafe], `read_item` [static], `read_step` [static], `read_worker` [static], `user_drop_static_step` [static,unsafe].

Accepted: `read_step_ok`, `read_worker_ok`.

## std (12)

Refused: `abort_item` [std], `drop_glue_item` [std], `foreach_step` [std], `foreach_worker` [std], `println_item` [std,unsafe], `println_step` [std,unsafe], `println_worker` [std], `thread_spawn_item` [std], `thread_spawn_step` [std], `thread_spawn_worker` [std].

Accepted: `foreach_worker_ok`, `option_expect_step_ok`.

## trusted (2)

Accepted: `math_exp_item_ok`, `math_f64_item_ok`.

## unsafe (19)

Refused: `block_item` [unsafe], `block_step` [unsafe], `block_worker` [unsafe], `cmp_max_user_ord_item` [unsafe], `direct_dependency` [error], `fn_item_into_core_item` [unsafe], `fn_item_output_max_item` [unsafe], `generic_closure_unsafe_step` [unsafe], `impl_trait_return_step` [unsafe], `ptr_read_method_item` [deny,unsafe], `sort_user_ord_item` [unsafe], `transmute_item` [unsafe], `unsafe_fn_step` [error], `user_clone_unsafe_step` [unsafe], `user_eq_unsafe_item` [unsafe], `user_iter_unsafe_item` [unbounded,unsafe].

Accepted: `block_item_ok`, `box_deref_step_ok`, `box_field_item_ok`.
