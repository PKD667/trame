//! `#[trame::state(steps(a, b), stop = f, pool = P, error = E)]` on a struct, and
//! `#[trame::step(S, receives = A, B, weight = 0.5)]` on a function (INTERFACE.md 5.3).
//!
//! `step` keeps the function and adds a braced struct of the same name (a braced struct lives only in
//! the type namespace, so it does not clash with the function). The struct carries the step's
//! `StepInfo` (its read and write sets as bits of the state's fields), its received types, and a
//! `__trame_run` that calls the function with its fields, projected from a raw pointer to the state.
//! A parameter that names no field, or a field of another type, is a compile error there.
//!
//! `state` keeps the struct, gives each field a bit (`S::__trame_f_<field>`), and implements
//! `trame::__rt::State` by dispatching on the step index with a `match`. No trait object and no
//! function pointer reaches a step.
//!
//! `step` also emits the marker the driver reads (check.rs): a hidden function of its struct that
//! calls `__check::step(&f)`, non-generic and never called; a checked build collects it. `state`
//! implements `trame::Plain` for the struct with a bound per field, so a field that is not plain is a
//! type error at that field (CONTRACT.md M2, M3); the pool is plain by `State::Pool`'s bound.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::parse::{ParseStream, Parser};
use syn::{Expr, FnArg, Ident, ItemFn, ItemStruct, Pat, Path, ReturnType, Token, Type};

/// `pool` is the parameter name that reads the host's pool instead of a field.
const POOL: &str = "pool";

struct StateArgs {
    steps: Vec<Path>,
    stop: Option<Path>,
    pool: Option<Type>,
    error: Option<Type>,
}

fn parse_state_args(attr: TokenStream2) -> syn::Result<StateArgs> {
    let mut out = StateArgs { steps: Vec::new(), stop: None, pool: None, error: None };
    let mut seen_steps = false;
    let parser = syn::meta::parser(|m| {
        if m.path.is_ident("steps") {
            seen_steps = true;
            m.parse_nested_meta(|s| {
                out.steps.push(s.path.clone());
                Ok(())
            })
        } else if m.path.is_ident("stop") {
            out.stop = Some(m.value()?.parse()?);
            Ok(())
        } else if m.path.is_ident("pool") {
            out.pool = Some(m.value()?.parse()?);
            Ok(())
        } else if m.path.is_ident("error") {
            out.error = Some(m.value()?.parse()?);
            Ok(())
        } else {
            Err(m.error("#[trame::state] takes steps(...), stop = f, pool = P, error = E"))
        }
    });
    parser.parse2(attr)?;
    if !seen_steps {
        return Err(syn::Error::new(Span::call_site(), "#[trame::state] needs steps(a, b, ...): the state's steps"));
    }
    Ok(out)
}

pub fn expand_state(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let args = parse_state_args(attr)?;
    let s: ItemStruct = syn::parse2(item)?;
    if !s.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&s.generics, "a state cannot be generic yet"));
    }
    let syn::Fields::Named(fields) = &s.fields else {
        return Err(syn::Error::new_spanned(&s.ident, "a state is a struct with named fields: steps name them"));
    };
    if fields.named.len() > 63 {
        return Err(syn::Error::new_spanned(
            &s.ident,
            "a state has at most 63 fields: each is one bit of a step's sets, and bit 63 is the pool",
        ));
    }
    let total = args.steps.len() + usize::from(args.stop.is_some());
    if total > 64 {
        return Err(syn::Error::new_spanned(&s.ident, "a state has at most 64 steps, the stop step included"));
    }
    let name = &s.ident;
    let vis = &s.vis;
    let consts = fields.named.iter().enumerate().map(|(k, f)| {
        let c = format_ident!("__trame_f_{}", f.ident.as_ref().expect("named field"));
        quote! { #[doc(hidden)] #vis const #c: u64 = 1u64 << #k; }
    });
    let all: Vec<&Path> = args.steps.iter().chain(args.stop.iter()).collect();
    let idx: Vec<usize> = (0..all.len()).collect();
    let error = args.error.map(|e| quote! { #e }).unwrap_or(quote! { ::core::convert::Infallible });
    let pool_ty = args.pool;
    let pool = pool_ty.as_ref().map(|p| quote! { #p }).unwrap_or(quote! { () });
    let has_stop = args.stop.is_some();
    let plain = plain_check(fields.named.iter());
    Ok(quote! {
        #s
        #[cfg(not(trame_checked))]
        compile_error!("build with cargo trame");
        // SAFETY: every field's type is plain, by the check below (CONTRACT.md M2, M3).
        unsafe impl ::trame::Plain for #name {}
        #plain
        #[allow(non_upper_case_globals)]
        impl #name {
            #(#consts)*
        }
        // SAFETY: each step's StepInfo is generated from its own parameters by #[trame::step], and its
        // __trame_run touches exactly those fields.
        unsafe impl ::trame::__rt::State for #name {
            type Error = #error;
            type Pool = #pool;
            const STEPS: &'static [::trame::__rt::StepInfo] = &[#(#all::__TRAME_INFO),*];
            const HAS_STOP: bool = #has_stop;
            fn frame_offered(i: usize) -> bool {
                match i {
                    #(#idx => #all::__trame_offered(),)*
                    _ => ::core::unreachable!("step index out of range"),
                }
            }
            fn tags(i: usize) -> ::trame::__rt::Tags {
                match i {
                    #(#idx => #all::__trame_tags(),)*
                    _ => ::core::unreachable!("step index out of range"),
                }
            }
            unsafe fn run(this: *mut Self, pool: &Self::Pool, i: usize) -> ::core::result::Result<::trame::Status, Self::Error> {
                match i {
                    // SAFETY: forwarded from the caller (State::run).
                    #(#idx => unsafe { #all::__trame_run(this, pool) },)*
                    _ => ::core::unreachable!("step index out of range"),
                }
            }
        }
    })
}

struct StepArgs {
    state: Type,
    receives: Vec<Type>,
    weight: Option<Expr>,
}

fn parse_step_args(attr: TokenStream2) -> syn::Result<StepArgs> {
    let parser = |input: ParseStream| -> syn::Result<StepArgs> {
        let state: Type = input.parse()?;
        let mut out = StepArgs { state, receives: Vec::new(), weight: None };
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "receives" {
                // Types up to the next `key =` or the end.
                loop {
                    out.receives.push(input.parse()?);
                    if input.is_empty() || (input.peek(Token![,]) && input.peek2(Ident) && input.peek3(Token![=])) {
                        break;
                    }
                    input.parse::<Token![,]>()?;
                }
            } else if key == "weight" {
                out.weight = Some(input.parse()?);
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    "#[trame::step] takes the state type, then receives = A, B and weight = w",
                ));
            }
        }
        Ok(out)
    };
    parser.parse2(attr)
}

pub fn expand_step(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let args = parse_step_args(attr)?;
    let f: ItemFn = syn::parse2(item)?;
    if !f.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&f.sig.generics, "a step cannot be generic"));
    }
    if f.sig.asyncness.is_some() || f.sig.unsafety.is_some() {
        return Err(syn::Error::new_spanned(&f.sig, "a step is a plain fn: not async, not unsafe"));
    }
    let st = &args.state;
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut call = Vec::new();
    for arg in &f.sig.inputs {
        let FnArg::Typed(pt) = arg else {
            return Err(syn::Error::new_spanned(arg, "a step takes fields of its state, not self"));
        };
        let Pat::Ident(p) = &*pt.pat else {
            return Err(syn::Error::new_spanned(
                &pt.pat,
                "a step parameter is the name of a field: `ring: &mut Vec<u32>`",
            ));
        };
        let field = &p.ident;
        let Type::Reference(r) = &*pt.ty else {
            return Err(syn::Error::new_spanned(
                &pt.ty,
                "a step parameter is `&T` (read) or `&mut T` (write) of its field's type",
            ));
        };
        if field == POOL {
            if r.mutability.is_some() {
                return Err(syn::Error::new_spanned(&pt.ty, "the pool is read-only: `pool: &P`"));
            }
            reads.push(quote! { ::trame::__rt::POOL });
            call.push(quote! { pool });
            continue;
        }
        let bit = format_ident!("__trame_f_{}", field);
        if r.mutability.is_some() {
            writes.push(quote! { <#st>::#bit });
            call.push(quote! { &mut *(&raw mut (*this).#field) });
        } else {
            reads.push(quote! { <#st>::#bit });
            call.push(quote! { &*(&raw const (*this).#field) });
        }
    }
    let fallible = match &f.sig.output {
        ReturnType::Type(_, t) => match &**t {
            Type::Path(p) => match p.path.segments.last().map(|s| s.ident.to_string()).as_deref() {
                Some("Status") => None,
                Some("Result") => Some(t.clone()),
                _ => return Err(syn::Error::new_spanned(t, "a step returns `Status` or `Result<Status, E>`")),
            },
            _ => return Err(syn::Error::new_spanned(t, "a step returns `Status` or `Result<Status, E>`")),
        },
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(&f.sig, "a step returns `Status` or `Result<Status, E>`"));
        }
    };
    let name = &f.sig.ident;
    let vis = &f.vis;
    let weight = args.weight.map(|w| quote! { #w }).unwrap_or(quote! { 0.0 });
    let recv = &args.receives;
    let n_recv = recv.len();
    let ridx: Vec<usize> = (0..n_recv).collect();
    let run = match fallible {
        None => quote! {
            #[inline(always)]
            pub unsafe fn __trame_run<E>(this: *mut #st, pool: &<#st as ::trame::__rt::State>::Pool) -> ::core::result::Result<::trame::Status, E> {
                let _ = pool;
                // SAFETY: the caller holds these fields (State::run).
                ::core::result::Result::Ok(unsafe { #name(#(#call),*) })
            }
        },
        Some(t) => quote! {
            #[inline(always)]
            pub unsafe fn __trame_run(this: *mut #st, pool: &<#st as ::trame::__rt::State>::Pool) -> #t {
                let _ = pool;
                // SAFETY: the caller holds these fields (State::run).
                unsafe { #name(#(#call),*) }
            }
        },
    };
    Ok(quote! {
        #f
        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #vis struct #name {
            _private: (),
        }
        impl #name {
            pub const __TRAME_INFO: ::trame::__rt::StepInfo = ::trame::__rt::StepInfo {
                name: ::core::stringify!(#name),
                reads: 0u64 #(| #reads)*,
                writes: 0u64 #(| #writes)*,
                weight: ::trame::__rt::Weight::from_f64(#weight),
            };
            pub fn __trame_offered() -> bool {
                false #(|| ::trame::__rt::offered(::core::any::TypeId::of::<#recv>()))*
            }
            pub fn __trame_tags() -> ::trame::__rt::Tags {
                let mut tags: ::trame::__rt::Tags = [::core::option::Option::None; ::trame::__rt::MAX_OWNED];
                #(tags[#ridx] = ::core::option::Option::Some((::core::any::TypeId::of::<#recv>(), ::core::any::type_name::<#recv>()));)*
                tags
            }
            #run
            #[allow(dead_code)]
            pub fn __trame_check() {
                ::trame::__check::step(&#name);
            }
        }
        const _: () = ::core::assert!(#n_recv <= ::trame::__rt::MAX_OWNED, "#[trame::step] receives more message types than MAX_OWNED (16)");
    })
}

/// A check that every field type of `fields` is plain, one call per field spanned at its type, so a
/// field that is not plain is reported where it is written. A function body, not a where-clause: the
/// impl it sits beside is unconditional, so a recursive type (`next: Option<Box<Node>>`) checks.
fn plain_check<'a>(fields: impl Iterator<Item = &'a syn::Field>) -> TokenStream2 {
    let calls = fields.map(|f| {
        let t = &f.ty;
        quote::quote_spanned! { syn::spanned::Spanned::span(t) => __trame_plain::<#t>(); }
    });
    quote! {
        const _: () = {
            fn __trame_plain<T: ::trame::Plain + ?Sized>() {}
            #[allow(dead_code)]
            fn __trame_fields() { #(#calls)* }
        };
    }
}

pub fn derive_plain(item: TokenStream2) -> syn::Result<TokenStream2> {
    let d: syn::DeriveInput = syn::parse2(item)?;
    if !d.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&d.generics, "#[derive(trame::Plain)] takes a type without generics"));
    }
    let fields: Vec<&syn::Field> = match &d.data {
        syn::Data::Struct(s) => s.fields.iter().collect(),
        syn::Data::Enum(e) => e.variants.iter().flat_map(|v| v.fields.iter()).collect(),
        syn::Data::Union(u) => {
            return Err(syn::Error::new_spanned(
                u.union_token,
                "a union is not plain: which field is live is not in its bytes",
            ));
        }
    };
    let check = plain_check(fields.into_iter());
    let name = &d.ident;
    Ok(quote! {
        // SAFETY: every field's type is plain, by the check below (CONTRACT.md M2, M3).
        unsafe impl ::trame::Plain for #name {}
        #check
    })
}
