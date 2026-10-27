//! `#[trame::process(receives = A, B)]` on `impl P { fn step(&mut self, a: &mut A, b: &mut B) -> Status }`
//! (spec.md 5.2, 5.3). The impl is kept as written. The macro adds `impl trame::__rt::Process for P`,
//! which names the cell types and owned message types of `step`, and runs `step` with its cells
//! borrowed from the runtime by type. The step's `Status`, or its `Result<Status, E>`, is the answer.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens};
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{FnArg, Ident, ImplItem, ItemImpl, PathArguments, ReturnType, Token, Type};

/// The primitive type names: a cell may not be one of them.
const PRIMITIVES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64",
    "bool", "char", "str",
];

/// The core and alloc types a cell may not be (spec.md 5.3: the cells are the step's own state).
const CORE_ALLOC: &[&str] = &["Vec", "VecDeque", "String", "Box", "Option", "Result"];

pub fn expand(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let receives = parse_receives(attr)?;
    let imp: ItemImpl = syn::parse2(item)?;
    if imp.trait_.is_some() {
        return Err(syn::Error::new_spanned(&imp.self_ty, "#[trame::process] goes on an inherent `impl P`, not a trait impl"));
    }
    if !imp.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&imp.generics, "a process type cannot be generic yet"));
    }
    let step = imp
        .items
        .iter()
        .find_map(|it| match it {
            ImplItem::Fn(f) if f.sig.ident == "step" => Some(f),
            _ => None,
        })
        .ok_or_else(|| syn::Error::new_spanned(&imp.self_ty, "a process needs `fn step(&mut self, ...) -> Status`"))?;
    let mut inputs = step.sig.inputs.iter();
    match inputs.next() {
        Some(FnArg::Receiver(r)) if r.mutability.is_some() && r.reference.is_some() => {}
        _ => return Err(syn::Error::new_spanned(&step.sig, "step takes `&mut self` first")),
    }
    let mut cells: Vec<(&Ident, &Type)> = Vec::new();
    for arg in inputs {
        let FnArg::Typed(pt) = arg else {
            return Err(syn::Error::new_spanned(arg, "step takes `&mut self` once, then cells"));
        };
        let name = match &*pt.pat {
            syn::Pat::Ident(p) => &p.ident,
            other => return Err(syn::Error::new_spanned(other, "a cell parameter is a plain name: `queue: &mut Queue`")),
        };
        let Type::Reference(r) = &*pt.ty else {
            return Err(syn::Error::new_spanned(&pt.ty, "a cell parameter is `&mut T`, where T is the cell's own type"));
        };
        if r.mutability.is_none() {
            return Err(syn::Error::new_spanned(&pt.ty, "a cell parameter is `&mut T`, not `&T`"));
        }
        refuse_cell(&step.sig.ident, name, &r.elem)?;
        if cells.iter().any(|(_, t)| t.to_token_stream().to_string() == r.elem.to_token_stream().to_string()) {
            return Err(syn::Error::new_spanned(&pt.ty, "two parameters of one type: a cell is matched by type, so give one of them a type of its own"));
        }
        cells.push((name, &r.elem));
    }
    let err = step_error(&step.sig.output)?;
    let self_ty = &imp.self_ty;
    let cell_tys: Vec<&Type> = cells.iter().map(|(_, t)| *t).collect();
    let vars: Vec<Ident> = (0..cells.len()).map(|k| quote::format_ident!("__cell{}", k)).collect();
    let arity = cells.len();
    let pick = if arity == 0 {
        quote! { let _ = cells; }
    } else {
        quote! { let (#(#vars,)*) = <(#(#cell_tys,)*) as ::trame::__rt::Pick<'_>>::pick(cells); }
    };
    let args = vars.iter().map(|v| quote! { #v });
    let call = quote! { self.step(#(#args),*) };
    let run_body = if step_is_result(&step.sig.output) {
        quote! { #pick #call }
    } else {
        quote! { #pick ::core::result::Result::Ok(#call) }
    };
    let owned_idx: Vec<usize> = (0..receives.len()).collect();
    let tags_n = receives.len();
    let frame_offered = if receives.is_empty() {
        quote! { false }
    } else {
        let tys = &receives;
        quote! { false #(|| ::trame::__rt::offered(::core::any::TypeId::of::<#tys>()))* }
    };
    Ok(quote! {
        #imp
        impl ::trame::__rt::Process for #self_ty {
            type Error = #err;
            const ARITY: usize = #arity;
            fn takes(ty: ::core::any::TypeId) -> bool {
                false #(|| ty == ::core::any::TypeId::of::<#cell_tys>())*
            }
            fn frame_offered() -> bool {
                #frame_offered
            }
            fn tags() -> ::trame::__rt::Tags {
                let mut __tags: ::trame::__rt::Tags = [None; ::trame::__rt::MAX_OWNED];
                #(__tags[#owned_idx] = Some((::core::any::TypeId::of::<#receives>(), ::core::any::type_name::<#receives>()));)*
                __tags
            }
            fn run(&mut self, cells: &mut ::trame::__rt::Cells<'_, '_>) -> ::core::result::Result<::trame::Status, #err> {
                let _owning = ::trame::__rt::Owning::new(<#self_ty as ::trame::__rt::Process>::tags());
                #run_body
            }
        }
        const _: () = assert!(#tags_n <= ::trame::__rt::MAX_OWNED, "#[trame::process] receives more message types than MAX_OWNED (16)");
    })
}

/// `receives = A, B`: the message types the process owns. Empty when the attribute is empty.
fn parse_receives(attr: TokenStream2) -> syn::Result<Vec<Type>> {
    if attr.is_empty() {
        return Ok(Vec::new());
    }
    let parser = |input: ParseStream| -> syn::Result<Punctuated<Type, Token![,]>> {
        let key: Ident = input.parse()?;
        if key != "receives" {
            return Err(syn::Error::new(key.span(), "#[trame::process] takes `receives = <message types>`"));
        }
        input.parse::<Token![=]>()?;
        Punctuated::parse_terminated(input)
    };
    Ok(parser.parse2(attr)?.into_iter().collect())
}

/// Refuse a cell type that is a primitive or a core/alloc type (spec.md 5.3).
fn refuse_cell(step: &Ident, param: &Ident, ty: &Type) -> syn::Result<()> {
    let bad = match ty {
        Type::Array(_) | Type::Slice(_) | Type::Tuple(_) | Type::Reference(_) => true,
        Type::Path(p) if p.qself.is_none() => {
            let segs = &p.path.segments;
            let first = segs.first().map(|s| s.ident.to_string()).unwrap_or_default();
            let last = segs.last().map(|s| s.ident.to_string()).unwrap_or_default();
            (segs.len() == 1 && PRIMITIVES.contains(&last.as_str()))
                || matches!(first.as_str(), "std" | "core" | "alloc")
                || CORE_ALLOC.contains(&last.as_str())
        }
        _ => false,
    };
    if bad {
        let shown = ty.to_token_stream().to_string();
        let msg = format!(
            "step `{step}`: parameter `{param}` has the type `{shown}`, a primitive or core/alloc type, which cannot be a cell. \
             Fix: wrap it in a type of your own, e.g. struct Pending(Vec<Job>)"
        );
        return Err(syn::Error::new_spanned(ty, msg));
    }
    Ok(())
}

/// The return type of step, if it is `Result<Status, E>`.
fn step_is_result(out: &ReturnType) -> bool {
    matches!(out, ReturnType::Type(_, t) if last_ident(t).as_deref() == Some("Result"))
}

/// The error type `E` of `Result<Status, E>`, or `Infallible` for a step that returns `Status`.
fn step_error(out: &ReturnType) -> syn::Result<TokenStream2> {
    let ReturnType::Type(_, ty) = out else {
        return Err(syn::Error::new_spanned(out, "step returns `Status` or `Result<Status, E>`"));
    };
    match &**ty {
        Type::Path(_) if last_ident(ty).as_deref() == Some("Status") => Ok(quote! { ::core::convert::Infallible }),
        Type::Path(p) if last_ident(ty).as_deref() == Some("Result") => {
            let seg = p.path.segments.last().expect("a path has a segment");
            let PathArguments::AngleBracketed(args) = &seg.arguments else {
                return Err(syn::Error::new_spanned(ty, "step returns `Result<Status, E>`"));
            };
            match args.args.iter().nth(1) {
                Some(syn::GenericArgument::Type(e)) => Ok(e.to_token_stream()),
                _ => Err(syn::Error::new_spanned(ty, "step returns `Result<Status, E>`")),
            }
        }
        _ => Err(syn::Error::new_spanned(ty, "step returns `Status` or `Result<Status, E>`")),
    }
}

fn last_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}
