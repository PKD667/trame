//! `#[message]`, `#[item]` and `parallel!`: the macros of trame's surface (spec.md 3, 5.4). Every
//! expansion names `::trame` and `::trame::__bytemuck`, so a user depends on trame alone. Only
//! the grammar accepted here is the surface; the driver checks what the expansions call.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
use quote::{format_ident, quote, ToTokens};
use syn::meta::ParseNestedMeta;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{
    parse_quote, Attribute, Expr, FnArg, Ident, ItemFn, ItemStruct, Pat, PathArguments, Token, Type,
};

/// Marks a struct as a message type (spec.md 3, "Messages"). It adds `#[repr(C)]`,
/// `Clone`, `Copy`, a check that the struct has no padding and that every bit pattern is valid
/// (by deriving bytemuck's `NoUninit` and `AnyBitPattern`), and `unsafe impl trame::Message`.
///
/// Refused: generic structs, enums, any `repr` other than `C` (`packed` and `transparent` skip
/// bytemuck's padding check), and a user `derive` of `Clone` or `Copy`.
#[proc_macro_attribute]
pub fn message(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_message(attr.into(), item).unwrap_or_else(|e| e.to_compile_error()).into()
}

fn expand_message(attr: TokenStream2, item: TokenStream) -> syn::Result<TokenStream2> {
    if !attr.is_empty() {
        return Err(syn::Error::new_spanned(attr, "#[trame::message] takes no arguments"));
    }
    let mut s: ItemStruct = syn::parse(item)?;
    if !s.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&s.generics, "a message type cannot be generic"));
    }
    for a in &s.attrs {
        check_attr(a)?;
    }
    for field in &s.fields {
        check_field_type(field.ty.to_token_stream())?;
    }
    let name = &s.ident;
    // `check_attr` admits only `repr(C)`, so a repr the user wrote already says what is wanted.
    if !s.attrs.iter().any(|a| a.path().is_ident("repr")) {
        s.attrs.insert(0, parse_quote!(#[repr(C)]));
    }
    s.attrs.push(parse_quote!(#[derive(::core::clone::Clone, ::core::marker::Copy, ::trame::__bytemuck::NoUninit, ::trame::__bytemuck::AnyBitPattern)]));
    s.attrs.push(parse_quote!(#[bytemuck(crate = "::trame::__bytemuck")]));
    Ok(quote! {
        #s
        unsafe impl ::trame::Message for #name {}
    })
}

/// A field type may not name a reference, a raw pointer or a function pointer (spec.md 3). The
/// check reads the tokens, so it sees through nesting (`[*const u8; 2]`) but not through a type
/// alias: an alias that hides a pointer is the implementer's promise to keep.
fn check_field_type(ts: TokenStream2) -> syn::Result<()> {
    let tts: Vec<TokenTree> = ts.into_iter().collect();
    for (i, tt) in tts.iter().enumerate() {
        match tt {
            TokenTree::Group(g) => check_field_type(g.stream())?,
            TokenTree::Punct(p) if p.as_char() == '&' => {
                return Err(syn::Error::new(p.span(), "a message field may not be a reference"));
            }
            // `*const T` and `*mut T` are raw pointers; `*` alone is multiplication, as in `[u8; 2 * 4]`.
            TokenTree::Punct(p)
                if p.as_char() == '*'
                    && matches!(tts.get(i + 1), Some(TokenTree::Ident(n)) if n == "const" || n == "mut") =>
            {
                return Err(syn::Error::new(p.span(), "a message field may not be a raw pointer"));
            }
            TokenTree::Ident(i) if i == "fn" => {
                return Err(syn::Error::new(i.span(), "a message field may not be a function pointer"));
            }
            _ => {}
        }
    }
    Ok(())
}

/// A user attribute on a message must not undo what the macro adds.
fn check_attr(a: &Attribute) -> syn::Result<()> {
    if a.path().is_ident("repr") {
        let mut ok = false;
        a.parse_nested_meta(|m: ParseNestedMeta| {
            ok = m.path.is_ident("C") && m.input.is_empty();
            if ok { Ok(()) } else { Err(m.error("a message type may only be `repr(C)`")) }
        })?;
        if !ok {
            return Err(syn::Error::new_spanned(a, "a message type may only be `repr(C)`"));
        }
    }
    if a.path().is_ident("derive") {
        a.parse_nested_meta(|m: ParseNestedMeta| {
            if m.path.is_ident("Clone") || m.path.is_ident("Copy") {
                Err(m.error("#[trame::message] derives Clone and Copy itself; remove this"))
            } else {
                Ok(())
            }
        })?;
    }
    Ok(())
}

/// Marks `fn f(x: &I, slot: &mut S, cx: &C)` as a `parallel!` item and generates the hidden key
/// function `__trame_key_f(x: &I) -> usize`, whose body is the given expression (spec.md 5.4).
/// The function itself is unchanged. The expression may name the first parameter.
///
/// `#[trame::item(key = x.index as usize)]`
#[proc_macro_attribute]
pub fn item(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_item(attr.into(), item).unwrap_or_else(|e| e.to_compile_error()).into()
}

fn expand_item(attr: TokenStream2, item: TokenStream) -> syn::Result<TokenStream2> {
    let mut key: Option<Expr> = None;
    let parser = syn::meta::parser(|m: ParseNestedMeta| {
        if m.path.is_ident("key") {
            key = Some(m.value()?.parse()?);
            Ok(())
        } else {
            Err(m.error("#[trame::item] takes only `key = <expr>`"))
        }
    });
    parser.parse2(attr)?;
    let key = key.ok_or_else(|| syn::Error::new(Span::call_site(), "#[trame::item] needs `key = <expr>`"))?;

    let f: ItemFn = syn::parse(item)?;
    if !f.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&f.sig.generics, "an item function cannot be generic yet"));
    }
    let inputs: Vec<&FnArg> = f.sig.inputs.iter().collect();
    let [FnArg::Typed(first), FnArg::Typed(second), FnArg::Typed(third)] = inputs.as_slice() else {
        return Err(syn::Error::new_spanned(&f.sig, "an item takes exactly (x: &I, slot: &mut S, cx: &C)"));
    };
    let Pat::Ident(first_name) = &*first.pat else {
        return Err(syn::Error::new_spanned(&first.pat, "the item's first parameter must be a plain name"));
    };
    if !matches!(&*first.ty, Type::Reference(r) if r.mutability.is_none()) {
        return Err(syn::Error::new_spanned(&first.ty, "the item's first parameter is `&I`"));
    }
    if !matches!(&*second.ty, Type::Reference(r) if r.mutability.is_some()) {
        return Err(syn::Error::new_spanned(&second.ty, "the item's second parameter is `&mut S`"));
    }
    if !matches!(&*third.ty, Type::Reference(r) if r.mutability.is_none()) {
        return Err(syn::Error::new_spanned(&third.ty, "the item's third parameter is `&C`"));
    }
    let key_fn = key_ident(&f.sig.ident);
    let (name, ty, vis) = (&first_name.ident, &first.ty, &f.vis);
    Ok(quote! {
        #f
        #[doc(hidden)]
        #vis fn #key_fn(#name: #ty) -> usize {
            #key
        }
    })
}

/// `parallel!(f, &cx, &items, &mut slots)`: apply the item `f` to `items` (spec.md 5.4). Answers `()`.
/// Aborts, before any item runs, when some item's key is not below `slots.len()`. Without
/// `--cfg trame_checked` (set by `cargo trame`) it is a compile error.
#[proc_macro]
pub fn parallel(input: TokenStream) -> TokenStream {
    expand_parallel(input.into()).unwrap_or_else(|e| e.to_compile_error()).into()
}

fn expand_parallel(input: TokenStream2) -> syn::Result<TokenStream2> {
    let args = Punctuated::<Expr, Token![,]>::parse_terminated.parse2(input)?;
    if args.len() != 4 {
        return Err(syn::Error::new_spanned(&args, "parallel! takes (f, &cx, &items, &mut slots)"));
    }
    let mut args = args.into_iter();
    let (f, cx, items, slots) = (args.next().unwrap(), args.next().unwrap(), args.next().unwrap(), args.next().unwrap());
    let Expr::Path(fp) = &f else {
        return Err(syn::Error::new_spanned(&f, "parallel!: the first argument is the name of an item function"));
    };
    let mut key_path = fp.clone();
    let last = key_path.path.segments.last_mut().expect("a path has a segment");
    if !matches!(last.arguments, PathArguments::None) {
        return Err(syn::Error::new_spanned(&f, "parallel!: an item function cannot be generic"));
    }
    last.ident = key_ident(&last.ident);
    Ok(quote! {
        {
            #[cfg(not(trame_checked))]
            compile_error!("build with cargo trame");
            #[cfg(trame_checked)]
            ::trame::__check::item(&#f);
            ::trame::__rt::parallel(#key_path, #f, #cx, #items, #slots)
        }
    })
}

mod concurrent;
mod process;

/// Marks an inherent `impl P` whose `step` is a `concurrent!` step (spec.md 5.2, 5.3):
/// `#[trame::process(receives = Job, Other)]`. Cell parameters are `&mut T` for the step's own types.
#[proc_macro_attribute]
pub fn process(attr: TokenStream, item: TokenStream) -> TokenStream {
    process::expand(attr.into(), item.into()).unwrap_or_else(|e| e.to_compile_error()).into()
}

/// `concurrent!(p0, p1; c0, c1)`: runs the processes over the cells (spec.md 5.3). Without
/// `--cfg trame_checked` (set by `cargo trame`) it is a compile error.
#[proc_macro]
pub fn concurrent(input: TokenStream) -> TokenStream {
    concurrent::expand(input.into()).unwrap_or_else(|e| e.to_compile_error()).into()
}

/// The hidden key function of item `f`.
fn key_ident(f: &Ident) -> Ident {
    format_ident!("__trame_key_{}", f)
}
