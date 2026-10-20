//! `#[component]`: a builder a template fills by property name.
//!
//! For `fn switch_row(label: &'static str, #[prop(default = "")] hint:
//! &'static str, on: fn(bool) -> Action)` it keeps the function as it is and
//! adds, beside it:
//!
//! - `__SwitchRowProps<L, H, O>`, one type parameter per property, each
//!   [`Unset`] until its setter turns it into [`Set`]. A setter takes the
//!   parameter's own type, so a literal or a closure is inferred as in a
//!   direct call, and exists only while its property is unset;
//! - per property a trait that hands the value over, implemented for `Set`
//!   and, when the property has a default, for `Unset` (the default is
//!   written beside the function, so its names resolve there);
//! - `__build`, which needs every property's trait and calls the function;
//! - a module named like the function holding `Props`, so whatever imports
//!   `switch_row` can also name `switch_row::Props`.
//!
//! `view!` turns `<SwitchRow on={f} label="x"/>` into
//! `switch_row::Props::__new().on(f).label("x").__build()`.
//!
//! [`Unset`]: nana_ui_runtime::view::__component::Unset
//! [`Set`]: nana_ui_runtime::view::__component::Set

use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::visit_mut::VisitMut;
use syn::{
    Expr, FnArg, GenericParam, Ident, ItemFn, Pat, Type, TypeImplTrait, Visibility, WherePredicate,
};

/// One parameter of the component.
struct Property {
    name: Ident,
    /// As written in the signature.
    ty: Type,
    /// The default, if the template may leave it out.
    default: Option<Expr>,
    /// Whether its type names the function's own generics: such a property
    /// is passed through as the type the call infers, and has no default.
    generic: bool,
}

pub(crate) fn expand(
    args: TokenStream,
    mut item: ItemFn,
    krate: &TokenStream,
) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "`#[component]` takes no arguments",
        ));
    }
    let signature = &item.sig;
    if let Some(asyncness) = signature.asyncness {
        return Err(syn::Error::new(
            asyncness.span(),
            "a component is not `async`",
        ));
    }
    if let Some(GenericParam::Lifetime(lifetime)) = signature
        .generics
        .params
        .iter()
        .find(|param| matches!(param, GenericParam::Lifetime(_)))
    {
        return Err(syn::Error::new(
            lifetime.span(),
            "a component takes owned or `'static` values: a template builds it once and keeps it",
        ));
    }
    let generics: HashSet<Ident> = signature
        .generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(param) => Some(param.ident.clone()),
            GenericParam::Const(param) => Some(param.ident.clone()),
            GenericParam::Lifetime(_) => None,
        })
        .collect();

    let mut properties = Vec::new();
    for input in &mut item.sig.inputs {
        let FnArg::Typed(input) = input else {
            return Err(syn::Error::new(
                input.span(),
                "a component is a free function",
            ));
        };
        let Pat::Ident(pat) = &*input.pat else {
            return Err(syn::Error::new(
                input.pat.span(),
                "a component's parameter needs a plain name: the template passes it by that name",
            ));
        };
        let name = pat.ident.clone();
        if name == "key" {
            return Err(syn::Error::new(
                name.span(),
                "`key` on a component tag keys its first root; name this parameter otherwise",
            ));
        }
        if name.to_string().starts_with("__") {
            return Err(syn::Error::new(
                name.span(),
                "a property's name cannot start with `__`",
            ));
        }
        let mut default = None;
        let mut kept = Vec::new();
        for attr in std::mem::take(&mut input.attrs) {
            if !attr.path().is_ident("prop") {
                kept.push(attr);
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("default") {
                    default = Some(if meta.input.peek(syn::Token![=]) {
                        meta.value()?.parse::<Expr>()?
                    } else {
                        syn::parse_quote_spanned!(meta.path.span()=> ::core::default::Default::default())
                    });
                    Ok(())
                } else {
                    Err(meta.error("`#[prop]` takes `default` or `default = expression`"))
                }
            })?;
        }
        input.attrs = kept;
        if let Some(lifetime) = borrowed(&input.ty) {
            return Err(syn::Error::new(
                lifetime,
                "a component takes owned or `'static` values: a template builds it once and keeps it",
            ));
        }
        let generic = names_any(&input.ty, &generics);
        if generic && default.is_some() {
            return Err(syn::Error::new(
                input.ty.span(),
                "a property whose type names the function's generics cannot have a default",
            ));
        }
        properties.push(Property {
            name,
            ty: (*input.ty).clone(),
            default,
            generic,
        });
    }

    let function = &item.sig.ident;
    let tag = pascal_case(&function.unraw().to_string());
    let props = format_ident!("__{tag}Props", span = function.span());
    let vis = &item.vis;
    let inner_vis = nested_visibility(vis);
    let unset = quote!(#krate::view::__component::Unset);
    let set = quote!(#krate::view::__component::Set);
    let states: Vec<Ident> = (0..properties.len())
        .map(|index| format_ident!("__S{index}"))
        .collect();
    let fields: Vec<&Ident> = properties.iter().map(|property| &property.name).collect();
    let traits: Vec<Ident> = properties
        .iter()
        .map(|property| format_ident!("{props}_{}", property.name.unraw()))
        .collect();
    let hidden =
        quote!(#[doc(hidden)] #[allow(dead_code, non_camel_case_types, clippy::type_complexity)]);

    let unsets = vec![unset.clone(); properties.len()];
    let mut out = TokenStream::new();
    out.extend(quote! {
        #hidden
        #[must_use = "a component's properties do nothing until `__build`"]
        #vis struct #props<#(#states),*> { #(#fields: #states),* }

        #hidden
        impl #props<#(#unsets),*> {
            pub fn __new() -> Self {
                #props { #(#fields: #unsets),* }
            }
        }
    });

    // Setters: one per property, while it is unset.
    for (index, property) in properties.iter().enumerate() {
        let name = &property.name;
        let mut ty = property.ty.clone();
        let mut impls = Desugar::new(&format!("__V{index}_"));
        impls.visit_type_mut(&mut ty);
        let mut params = impls.params();
        if property.generic {
            params.extend(own_generics(&item.sig.generics, &property.ty, &generics));
        }
        let predicates = if property.generic {
            own_predicates(&item.sig.generics, &property.ty, &generics)
        } else {
            Vec::new()
        };
        let others: Vec<&Ident> = states
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, state)| state)
            .collect();
        let before = states.iter().enumerate().map(|(other, state)| {
            if other == index {
                unset.clone()
            } else {
                quote!(#state)
            }
        });
        let after = states.iter().enumerate().map(|(other, state)| {
            if other == index {
                quote!(#set<#ty>)
            } else {
                quote!(#state)
            }
        });
        let moved = fields.iter().enumerate().map(|(other, field)| {
            if other == index {
                quote!(#field: #set(value))
            } else {
                quote!(#field: self.#field)
            }
        });
        out.extend(quote_spanned! {name.span()=>
            #hidden
            impl<#(#others),*> #props<#(#before),*> {
                pub fn #name<#(#params),*>(self, value: #ty) -> #props<#(#after),*>
                where #(#predicates,)*
                {
                    #props { #(#moved),* }
                }
            }
        });
    }

    // How each property reaches the call: its value, or its default.
    let message_tag = tag.clone();
    for (index, property) in properties.iter().enumerate() {
        let name = property.name.unraw().to_string();
        let trait_name = &traits[index];
        let message = format!("`<{message_tag}>` needs `{name}`");
        let label = format!("`{name}` is not given");
        let note = format!("write `{name}=…` on `<{message_tag}>`");
        let diagnostic = quote! {
            #[diagnostic::on_unimplemented(message = #message, label = #label, note = #note)]
        };
        if property.generic {
            out.extend(quote! {
                #hidden
                #diagnostic
                #vis trait #trait_name<__T> { fn __take(self) -> __T; }
                #hidden
                impl<__T> #trait_name<__T> for #set<__T> { fn __take(self) -> __T { self.0 } }
            });
            continue;
        }
        let ty = &property.ty;
        let mut concrete = property.ty.clone();
        let mut impls = Desugar::new("__V");
        impls.visit_type_mut(&mut concrete);
        let params = impls.params();
        out.extend(quote! {
            #hidden
            #diagnostic
            #vis trait #trait_name { fn __take(self) -> #ty; }
            #hidden
            impl<#(#params),*> #trait_name for #set<#concrete> { fn __take(self) -> #ty { self.0 } }
        });
        if let Some(default) = &property.default {
            out.extend(quote_spanned! {default.span()=>
                #hidden
                impl #trait_name for #unset { fn __take(self) -> #ty { #default } }
            });
        }
    }

    // The call.
    let mut build_params: Vec<TokenStream> = item
        .sig
        .generics
        .params
        .iter()
        .map(|param| match param {
            GenericParam::Type(param) => {
                let mut param = param.clone();
                param.default = None;
                quote!(#param)
            }
            GenericParam::Const(param) => {
                let mut param = param.clone();
                param.default = None;
                param.eq_token = None;
                quote!(#param)
            }
            GenericParam::Lifetime(param) => quote!(#param),
        })
        .collect();
    let mut bounds: Vec<TokenStream> = item
        .sig
        .generics
        .where_clause
        .iter()
        .flat_map(|clause| clause.predicates.iter())
        .map(|predicate| quote!(#predicate))
        .collect();
    let mut args = Vec::new();
    for (index, property) in properties.iter().enumerate() {
        let state = &states[index];
        let trait_name = &traits[index];
        let field = &property.name;
        if property.generic {
            let mut ty = property.ty.clone();
            let mut impls = Desugar::new(&format!("__W{index}_"));
            impls.visit_type_mut(&mut ty);
            build_params.extend(impls.params());
            bounds.push(quote!(#state: #trait_name<#ty>));
            args.push(quote!(<#state as #trait_name<#ty>>::__take(self.#field)));
        } else {
            bounds.push(quote!(#state: #trait_name));
            args.push(quote!(<#state as #trait_name>::__take(self.#field)));
        }
    }
    let output = &item.sig.output;
    out.extend(quote! {
        #hidden
        impl<#(#states),*> #props<#(#states),*> {
            pub fn __build<#(#build_params),*>(self) #output
            where #(#bounds,)*
            {
                #function(#(#args),*)
            }
        }

        #hidden
        #vis mod #function {
            #inner_vis use super::#props as Props;
        }
    });

    Ok(quote!(#item #out))
}

/// `switch_row` → `SwitchRow`.
fn pascal_case(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

/// The visibility of an item inside a module nested in the function's own,
/// that reaches as far as the function does.
fn nested_visibility(vis: &Visibility) -> TokenStream {
    match vis {
        Visibility::Public(_) => quote!(pub),
        Visibility::Inherited => quote!(pub(super)),
        Visibility::Restricted(restricted) => {
            let path = &restricted.path;
            let first = path
                .segments
                .first()
                .map(|segment| segment.ident.to_string());
            match first.as_deref() {
                Some("crate") => quote!(#vis),
                Some("self") if path.segments.len() == 1 => quote!(pub(super)),
                Some("self") => {
                    let rest = path.segments.iter().skip(1);
                    quote!(pub(in super #(::#rest)*))
                }
                _ => quote!(pub(in super::#path)),
            }
        }
    }
}

/// The span of a borrow that is not `'static`, if `ty` has one.
fn borrowed(ty: &Type) -> Option<Span> {
    struct Finder(Option<Span>);
    impl<'ast> Visit<'ast> for Finder {
        fn visit_type_reference(&mut self, reference: &'ast syn::TypeReference) {
            match &reference.lifetime {
                Some(lifetime) if lifetime.ident == "static" => {}
                _ => {
                    self.0.get_or_insert(reference.and_token.span);
                }
            }
            syn::visit::visit_type_reference(self, reference);
        }
        // `impl Fn(&str)` and `fn(&str)` borrow only for the call.
        fn visit_type_bare_fn(&mut self, _: &'ast syn::TypeBareFn) {}
        fn visit_parenthesized_generic_arguments(
            &mut self,
            _: &'ast syn::ParenthesizedGenericArguments,
        ) {
        }
    }
    let mut finder = Finder(None);
    finder.visit_type(ty);
    finder.0
}

/// Whether `ty` names any of `generics`.
fn names_any(ty: &Type, generics: &HashSet<Ident>) -> bool {
    mentioned(&quote!(#ty), generics)
}

fn mentioned(tokens: &TokenStream, generics: &HashSet<Ident>) -> bool {
    tokens.clone().into_iter().any(|tree| match tree {
        proc_macro2::TokenTree::Ident(ident) => generics.contains(&ident),
        proc_macro2::TokenTree::Group(group) => mentioned(&group.stream(), generics),
        _ => false,
    })
}

/// The function's generic parameters `ty` names, with their bounds.
fn own_generics(generics: &syn::Generics, ty: &Type, all: &HashSet<Ident>) -> Vec<TokenStream> {
    let named: HashSet<Ident> = all
        .iter()
        .filter(|ident| mentioned(&quote!(#ty), &HashSet::from([(*ident).clone()])))
        .cloned()
        .collect();
    generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(param) if named.contains(&param.ident) => {
                let mut param = param.clone();
                param.default = None;
                Some(quote!(#param))
            }
            GenericParam::Const(param) if named.contains(&param.ident) => {
                let mut param = param.clone();
                param.default = None;
                param.eq_token = None;
                Some(quote!(#param))
            }
            _ => None,
        })
        .collect()
}

/// The function's `where` predicates that bound only generics `ty` names.
fn own_predicates(
    generics: &syn::Generics,
    ty: &Type,
    all: &HashSet<Ident>,
) -> Vec<WherePredicate> {
    let named: HashSet<Ident> = all
        .iter()
        .filter(|ident| mentioned(&quote!(#ty), &HashSet::from([(*ident).clone()])))
        .cloned()
        .collect();
    let others: HashSet<Ident> = all.difference(&named).cloned().collect();
    generics
        .where_clause
        .iter()
        .flat_map(|clause| clause.predicates.iter())
        .filter(|predicate| {
            let tokens = quote!(#predicate);
            mentioned(&tokens, &named) && !mentioned(&tokens, &others)
        })
        .cloned()
        .collect()
}

/// Replaces each `impl Bounds` in a type with a fresh type parameter, and
/// remembers the parameters with their bounds.
struct Desugar {
    prefix: String,
    found: Vec<(Ident, TypeImplTrait)>,
}

impl Desugar {
    fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_owned(),
            found: Vec::new(),
        }
    }

    fn params(&self) -> Vec<TokenStream> {
        self.found
            .iter()
            .map(|(ident, impl_trait)| {
                let bounds = &impl_trait.bounds;
                quote!(#ident: #bounds)
            })
            .collect()
    }
}

impl VisitMut for Desugar {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::ImplTrait(impl_trait) = ty {
            let mut impl_trait = impl_trait.clone();
            // `impl Fn(impl X)` is not Rust; the bounds stay as written.
            let ident = format_ident!(
                "{}{}",
                self.prefix,
                self.found.len(),
                span = impl_trait.span()
            );
            impl_trait.impl_token = Default::default();
            *ty = syn::parse_quote!(#ident);
            self.found.push((ident, impl_trait));
            return;
        }
        syn::visit_mut::visit_type_mut(self, ty);
    }
}
