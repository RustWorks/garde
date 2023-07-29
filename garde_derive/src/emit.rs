use std::cell::RefCell;

use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::{format_ident, quote, quote_spanned, ToTokens};
use syn::spanned::Spanned;

use crate::model;

pub fn emit(input: model::Validate) -> TokenStream2 {
    input.to_token_stream()
}

impl ToTokens for model::Validate {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let ident = &self.ident;
        let context_ty = &self.context;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        let kind = &self.kind;

        quote! {
            impl #impl_generics ::garde::Validate for #ident #ty_generics #where_clause {
                type Context = #context_ty ;

                #[allow(clippy::needless_borrow)]
                fn validate(&self, __garde_user_ctx: &Self::Context) -> ::core::result::Result<(), ::garde::error::Errors> {
                    (
                        #kind
                    )
                    .finish()
                }
            }
        }
        .to_tokens(tokens)
    }
}

impl ToTokens for model::ValidateKind {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        match self {
            model::ValidateKind::Struct(variant) => {
                let bindings = Bindings(variant);
                let validation = Validation(variant);

                quote! {{
                    let Self #bindings = self;
                    #validation
                }}
            }
            model::ValidateKind::Enum(variants) => {
                let variants = variants.iter().map(|(name, variant)| {
                    let bindings = Bindings(variant);
                    let validation = Validation(variant);

                    quote!(Self::#name #bindings => #validation)
                });

                quote! {{
                    match self {
                        #(#variants,)*
                    }
                }}
            }
        }
        .to_tokens(tokens)
    }
}

struct Validation<'a>(&'a model::ValidateVariant);

impl<'a> ToTokens for Validation<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        // TODO: deduplicate this a bit
        match &self.0 {
            model::ValidateVariant::Struct(fields) => {
                let fields = Struct(fields);
                quote! {
                    ::garde::error::Errors::fields(|__garde_errors| {#fields})
                }
            }
            model::ValidateVariant::Tuple(fields) => {
                let fields = Tuple(fields);
                quote! {
                    ::garde::error::Errors::list(|__garde_errors| {#fields})
                }
            }
        }
        .to_tokens(tokens)
    }
}

struct Struct<'a>(&'a [(Ident, model::ValidateField)]);

impl<'a> ToTokens for Struct<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        Fields::new(
            self.0
                .iter()
                .map(|(key, field)| (Binding::Ident(key), field, key.to_string())),
            |key, value| {
                quote! {
                    __garde_errors.insert(
                        #key,
                        #value,
                    );
                }
            },
        )
        .to_tokens(tokens)
    }
}

struct Tuple<'a>(&'a [model::ValidateField]);

impl<'a> ToTokens for Tuple<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        Fields::new(
            self.0
                .iter()
                .enumerate()
                .map(|(index, field)| (Binding::Index(index), field, ())),
            |(), value| {
                quote! {
                    __garde_errors.push(#value);
                }
            },
        )
        .to_tokens(tokens)
    }
}

struct Inner<'a>(&'a model::RuleSet);

impl<'a> ToTokens for Inner<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let Inner(rule_set) = self;

        let outer = match rule_set.has_top_level_rules() {
            true => {
                let rules = Rules(rule_set);
                Some(quote! {
                    ::garde::error::Errors::simple(
                        |__garde_errors| {
                            #rules
                        }
                    )
                })
            }
            false => None,
        };
        let inner = rule_set.inner.as_deref().map(Inner);

        let value = match (outer, inner) {
            (Some(outer), Some(inner)) => quote! {
                    ::garde::error::Errors::nested(
                        #outer,
                        #inner,
                    )
            },
            (None, Some(inner)) => quote! {
                    ::garde::error::Errors::nested(
                        ::garde::error::Errors::empty(),
                        #inner,
                    )
            },
            (Some(outer), None) => outer,
            (None, None) => return,
        };

        quote! {
            ::garde::rules::inner::apply(
                &*__garde_binding,
                __garde_user_ctx,
                |__garde_binding, __garde_user_ctx| {
                    #value
                }
            )
        }
        .to_tokens(tokens)
    }
}

struct Rules<'a>(&'a model::RuleSet);

#[derive(Clone, Copy)]
enum Binding<'a> {
    Ident(&'a Ident),
    Index(usize),
}

impl<'a> ToTokens for Binding<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        match self {
            Binding::Ident(v) => v.to_tokens(tokens),
            Binding::Index(v) => format_ident!("_{v}").to_tokens(tokens),
        }
    }
}

impl<'a> ToTokens for Rules<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let Rules(rule_set) = self;

        for custom_rule in rule_set.custom_rules.iter() {
            quote! {
                if let Err(__garde_error) = (#custom_rule)(&*__garde_binding, &__garde_user_ctx) {
                    __garde_errors.push(__garde_error)
                }
            }
            .to_tokens(tokens)
        }

        for rule in rule_set.rules.iter() {
            let name = format_ident!("{}", rule.name());
            use model::ValidateRule::*;
            let args = match rule {
                Ascii | Alphanumeric | Email | Url | CreditCard | PhoneNumber | Required => {
                    quote!(())
                }
                Ip => {
                    quote!((::garde::rules::ip::IpKind::Any,))
                }
                IpV4 => {
                    quote!((::garde::rules::ip::IpKind::V4,))
                }
                IpV6 => {
                    quote!((::garde::rules::ip::IpKind::V6,))
                }
                Length(range) | ByteLength(range) => match range {
                    model::ValidateRange::GreaterThan(min) => quote!((#min, usize::MAX)),
                    model::ValidateRange::LowerThan(max) => quote!((0usize, #max)),
                    model::ValidateRange::Between(min, max) => quote!((#min, #max)),
                },
                Range(range) => match range {
                    model::ValidateRange::GreaterThan(min) => quote!((Some(#min), None)),
                    model::ValidateRange::LowerThan(max) => quote!((None, Some(#max))),
                    model::ValidateRange::Between(min, max) => quote!((Some(#min), Some(#max))),
                },
                Contains(expr) | Prefix(expr) | Suffix(expr) => {
                    quote_spanned!(expr.span() => (&#expr,))
                }
                Pattern(pat) => match pat {
                    model::ValidatePattern::Expr(expr) => quote_spanned!(expr.span() => (&#expr,)),
                    model::ValidatePattern::Lit(s) => quote!({
                        static PATTERN: ::garde::rules::pattern::StaticPattern =
                            ::garde::rules::pattern::init_pattern!(#s);
                        (&PATTERN,)
                    }),
                },
            };
            quote! {
                if let Err(__garde_error) = (::garde::rules::#name::apply)(&*__garde_binding, #args) {
                    __garde_errors.push(__garde_error)
                }
            }
            .to_tokens(tokens)
        }
    }
}

struct Fields<I, F>(RefCell<Option<I>>, F);

impl<I, F> Fields<I, F> {
    fn new(iter: I, f: F) -> Self {
        Self(RefCell::new(Some(iter)), f)
    }
}

impl<'a, I, F, Extra> ToTokens for Fields<I, F>
where
    I: Iterator<Item = (Binding<'a>, &'a model::ValidateField, Extra)> + 'a,
    F: Fn(Extra, TokenStream2) -> TokenStream2,
{
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let fields = match self.0.borrow_mut().take() {
            Some(v) => v,
            None => return,
        };
        let fields = fields.filter(|(_, field, _)| field.skip.is_none());
        for (binding, field, extra) in fields {
            let rules = Rules(&field.rule_set);
            let outer = match field.has_top_level_rules() {
                true => Some(quote! {
                    ::garde::error::Errors::simple(|__garde_errors| {#rules})
                }),
                false => None,
            };
            let inner = match (&field.dive, &field.rule_set.inner) {
                (Some(..), None) => Some(quote! {
                    ::garde::validate::Validate::validate(
                        &*__garde_binding,
                        __garde_user_ctx,
                    )
                        .err()
                        .unwrap_or_else(::garde::error::Errors::empty)
                }),
                (None, Some(inner)) => {
                    let inner = Inner(inner);
                    Some(inner.to_token_stream())
                }
                (None, None) => None,
                // TODO: encode this via the type system instead?
                _ => unreachable!("`dive` and `inner` are mutually exclusive"),
            };

            let value = match (outer, inner) {
                (Some(outer), Some(inner)) => quote! {
                    {
                        let __garde_binding = &*#binding;
                        ::garde::error::Errors::nested(
                            #outer,
                            #inner,
                        )
                    }
                },
                (None, Some(inner)) => quote! {
                    {
                        let __garde_binding = &*#binding;
                        ::garde::error::Errors::nested(
                            ::garde::error::Errors::empty(),
                            #inner,
                        )
                    }
                },
                (Some(outer), None) => quote! {
                    {
                        let __garde_binding = &*#binding;
                        #outer
                    }
                },
                (None, None) => unreachable!("field should already be skipped"),
            };

            let add = &self.1;

            add(extra, value).to_tokens(tokens)
        }
    }
}

struct Bindings<'a>(&'a model::ValidateVariant);

impl<'a> ToTokens for Bindings<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        match &self.0 {
            model::ValidateVariant::Struct(fields) => {
                let names = fields
                    .iter()
                    .filter(|field| field.1.skip.is_none())
                    .map(|field| &field.0)
                    .collect::<Vec<_>>();
                let rest = if names.len() != fields.len() {
                    Some(quote!(..))
                } else {
                    None
                };

                quote!( { #(#names,)* #rest } )
            }
            model::ValidateVariant::Tuple(fields) => {
                let indices = fields
                    .iter()
                    .enumerate()
                    .filter(|(_, field)| field.skip.is_none())
                    .map(|(i, _)| IndexBinding(i))
                    .collect::<Vec<_>>();
                let rest = if indices.len() != fields.len() {
                    Some(quote!(..))
                } else {
                    None
                };

                quote!( ( #(#indices,)* #rest ) )
            }
        }
        .to_tokens(tokens)
    }
}

struct IndexBinding(usize);
impl ToTokens for IndexBinding {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        format_ident!("_{}", self.0).to_tokens(tokens)
    }
}
