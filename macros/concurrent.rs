//! `concurrent!(p0, p1; c0, c1)` (spec.md 5.3): runs the processes `p0, p1` (values of process types,
//! in priority order) over the cells `c0, c1` (variables, borrowed mutably for the run). Answers
//! `Result<(), E>`. Without `--cfg trame_checked` it is a compile error.
//!
//! The expansion generates a `Set` over the processes, with a `match` on the process index, and
//! calls the runtime `trame::__rt::concurrent`. The driver sees each process's `step` through the
//! `trame::__check::step` marker, which is compiled only in a checked build.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Expr, Index, Token};

pub fn expand(input: TokenStream2) -> syn::Result<TokenStream2> {
    let parser = |input: ParseStream| -> syn::Result<(Punctuated<Expr, Token![,]>, Punctuated<Expr, Token![,]>)> {
        let procs = Punctuated::parse_separated_nonempty(input)?;
        input.parse::<Token![;]>()?;
        let cells = Punctuated::parse_terminated(input)?;
        Ok((procs, cells))
    };
    let (procs, cells): (Vec<Expr>, Vec<Expr>) = {
        let (p, c) = parser.parse2(input)?;
        (p.into_iter().collect(), c.into_iter().collect())
    };
    if procs.len() > 64 {
        return Err(syn::Error::new_spanned(&procs[64], "concurrent! runs at most 64 processes"));
    }
    let gens: Vec<_> = (0..procs.len()).map(|i| format_ident!("__P{}", i)).collect();
    let idx: Vec<usize> = (0..procs.len()).collect();
    let fields: Vec<Index> = idx.iter().map(|&i| Index::from(i)).collect();
    let n = procs.len();
    let c = cells.len();
    let first = &gens[0];
    let bounds: Vec<TokenStream2> = gens
        .iter()
        .enumerate()
        .map(|(i, g)| {
            if i == 0 {
                quote! { #g: ::trame::__rt::Process }
            } else {
                quote! { #g: ::trame::__rt::Process<Error = <#first as ::trame::__rt::Process>::Error> }
            }
        })
        .collect();
    Ok(quote! {
        {
            #[cfg(not(trame_checked))]
            compile_error!("build with cargo trame");
            #[cfg(trame_checked)]
            {
                #(::trame::__check::step(&#procs);)*
            }
            struct __Set<#(#gens),*>(#(#gens),*);
            impl<#(#bounds),*> ::trame::__rt::Set for __Set<#(#gens),*> {
                type Error = <#first as ::trame::__rt::Process>::Error;
                const LEN: usize = #n;
                fn name(i: usize) -> &'static str {
                    match i {
                        #(#idx => ::core::any::type_name::<#gens>(),)*
                        _ => unreachable!("process index out of range"),
                    }
                }
                fn takes(i: usize, ty: ::core::any::TypeId) -> bool {
                    match i {
                        #(#idx => <#gens as ::trame::__rt::Process>::takes(ty),)*
                        _ => unreachable!("process index out of range"),
                    }
                }
                fn arity(i: usize) -> usize {
                    match i {
                        #(#idx => <#gens as ::trame::__rt::Process>::ARITY,)*
                        _ => unreachable!("process index out of range"),
                    }
                }
                fn frame_offered(i: usize) -> bool {
                    match i {
                        #(#idx => <#gens as ::trame::__rt::Process>::frame_offered(),)*
                        _ => unreachable!("process index out of range"),
                    }
                }
                fn tags(i: usize) -> ::trame::__rt::Tags {
                    match i {
                        #(#idx => <#gens as ::trame::__rt::Process>::tags(),)*
                        _ => unreachable!("process index out of range"),
                    }
                }
                fn run(&mut self, i: usize, cells: &mut ::trame::__rt::Cells<'_, '_>) -> ::core::result::Result<::trame::Status, Self::Error> {
                    match i {
                        #(#idx => <#gens as ::trame::__rt::Process>::run(&mut self.#fields, cells),)*
                        _ => unreachable!("process index out of range"),
                    }
                }
            }
            let mut __procs = __Set(#(#procs),*);
            let __tys: [(::core::any::TypeId, &'static str); #c] = [#(::trame::__rt::cell_ty(&#cells)),*];
            let mut __refs: [&mut dyn ::core::any::Any; #c] = [#(&mut #cells),*];
            let mut __cells = ::trame::__rt::Cells::new(&mut __refs, &__tys);
            ::trame::__rt::concurrent(&mut __procs, &mut __cells)
        }
    })
}
