//! `#[derive(trame::Object)]`: the four methods of `trame::Object` for a struct or enum whose fields are objects.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Index};

/// The accessors of a field list: for each field, the member that names it in a struct literal or a pattern, and its
/// type.
fn members(fields: &Fields) -> Vec<(syn::Member, &syn::Type)> {
    fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            (f.ident.clone().map(syn::Member::Named).unwrap_or_else(|| syn::Member::Unnamed(Index::from(i))), &f.ty)
        })
        .collect()
}

fn name_of(m: &syn::Member) -> String {
    match m {
        syn::Member::Named(i) => i.to_string(),
        syn::Member::Unnamed(i) => i.index.to_string(),
    }
}

pub fn derive_object(input: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&input.generics, "an object type cannot be generic"));
    }
    let name = &input.ident;
    let label = name.to_string();
    let (size, encode, decode, shape) = match &input.data {
        Data::Struct(s) => {
            let ms = members(&s.fields);
            let mem: Vec<_> = ms.iter().map(|(m, _)| m).collect();
            let ty: Vec<_> = ms.iter().map(|(_, t)| *t).collect();
            let fname: Vec<_> = ms.iter().map(|(m, _)| name_of(m)).collect();
            (
                quote! { 0usize #( + ::trame::Object::size(&self.#mem) )* },
                quote! { #( ::trame::Object::encode(&self.#mem, w); )* },
                quote! { ::core::result::Result::Ok(#name { #( #mem: <#ty as ::trame::Object>::decode(r)? ),* }) },
                quote! { s.word("struct"); s.word(#label); #( s.word(#fname); <#ty as ::trame::Object>::shape(s); )* },
            )
        }
        Data::Enum(e) => {
            let mut sizes = Vec::new();
            let mut encodes = Vec::new();
            let mut decodes = Vec::new();
            let mut shapes = Vec::new();
            for (i, v) in e.variants.iter().enumerate() {
                let (vname, i) = (&v.ident, i as u32);
                let ms = members(&v.fields);
                let bind: Vec<_> = (0..ms.len()).map(|k| format_ident!("f{k}")).collect();
                let mem: Vec<_> = ms.iter().map(|(m, _)| m).collect();
                let ty: Vec<_> = ms.iter().map(|(_, t)| *t).collect();
                let fname: Vec<_> = ms.iter().map(|(m, _)| name_of(m)).collect();
                let pattern = quote! { #name::#vname { #( #mem: #bind ),* } };
                let vlabel = vname.to_string();
                sizes.push(quote! { #pattern => 4usize #( + ::trame::Object::size(#bind) )* });
                encodes.push(quote! { #pattern => { w.put_u32(#i); #( ::trame::Object::encode(#bind, w); )* } });
                decodes.push(quote! { #i => ::core::result::Result::Ok(#name::#vname { #( #mem: <#ty as ::trame::Object>::decode(r)? ),* }) });
                shapes.push(quote! { s.word(#vlabel); #( s.word(#fname); <#ty as ::trame::Object>::shape(s); )* });
            }
            if e.variants.is_empty() {
                return Err(syn::Error::new_spanned(&input.ident, "an object enum needs a variant"));
            }
            (
                quote! { match self { #( #sizes ),* } },
                quote! { match self { #( #encodes )* } },
                quote! {
                    match r.get_u32()? {
                        #( #decodes, )*
                        _ => ::core::result::Result::Err(::trame::Malformed("an enum tag that names no variant")),
                    }
                },
                quote! { s.word("enum"); s.word(#label); #( #shapes )* },
            )
        }
        Data::Union(_) => return Err(syn::Error::new_spanned(&input.ident, "a union cannot be an object")),
    };
    Ok(quote! {
        impl ::trame::Object for #name {
            fn size(&self) -> usize { #size }
            fn encode(&self, w: &mut ::trame::__object::Writer) { #encode }
            fn decode(r: &mut ::trame::__object::Reader<'_>) -> ::core::result::Result<Self, ::trame::Malformed> { #decode }
            fn shape(s: &mut ::trame::__object::Shape) { #shape }
        }
    })
}
