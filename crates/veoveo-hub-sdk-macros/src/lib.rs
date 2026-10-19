//! Thin authoring syntax over ordinary public SDK construction traits.
use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use std::collections::BTreeMap;
use syn::{
    DeriveInput, Expr, Token, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

#[proc_macro_derive(SingleOutput)]
pub fn single_output(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    if !matches!(input.data, syn::Data::Struct(_) | syn::Data::Enum(_)) {
        return syn::Error::new_spanned(
            input.ident,
            "a single output must be an explicit record or enum",
        )
        .to_compile_error()
        .into();
    }
    let name = input.ident;
    let (implementation, arguments, where_clause) = input.generics.split_for_impl();
    quote!(impl #implementation ::veoveo_hub_sdk::definition::SingleOutput for #name #arguments #where_clause {}).into()
}

enum Argument {
    Type(Type),
    Expression(Expr),
}
impl ToTokens for Argument {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Type(value) => value.to_tokens(tokens),
            Self::Expression(value) => value.to_tokens(tokens),
        }
    }
}
struct Arguments(BTreeMap<String, Argument>);
impl Parse for Arguments {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut result = BTreeMap::new();
        while !input.is_empty() {
            let name: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let key = name.to_string();
            let value = if matches!(
                key.as_str(),
                "provider" | "output" | "content" | "collection" | "address_type"
            ) {
                Argument::Type(input.parse()?)
            } else {
                Argument::Expression(input.parse()?)
            };
            if key == "task"
                && !matches!(
                    &value,
                    Argument::Expression(Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Bool(_),
                        ..
                    }))
                )
            {
                return Err(syn::Error::new_spanned(value, "task must be true or false"));
            }
            if result.insert(key, value).is_some() {
                return Err(syn::Error::new_spanned(
                    name,
                    "duplicate declaration argument",
                ));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self(result))
    }
}
impl Arguments {
    fn take(&mut self, name: &str) -> syn::Result<Argument> {
        self.0.remove(name).ok_or_else(|| {
            syn::Error::new(proc_macro2::Span::call_site(), format!("missing {name}"))
        })
    }
    fn done(self) -> syn::Result<()> {
        if let Some((name, value)) = self.0.into_iter().next() {
            Err(syn::Error::new_spanned(
                value,
                format!("unknown argument {name}"),
            ))
        } else {
            Ok(())
        }
    }
}
fn expand(
    attribute: TokenStream,
    input: TokenStream,
    kind: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut args: Arguments = syn::parse(attribute)?;
    let input: syn::ItemStruct = syn::parse(input)?;
    let name = &input.ident;
    let (implementation, generics, where_clause) = input.generics.split_for_impl();
    let provider = args.take("provider")?;
    let generated = match kind {
        "tool" => {
            let output = args.take("output")?;
            let tool = args.take("name")?;
            let task = match args.0.remove("task") {
                Some(Argument::Expression(Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Bool(value),
                    ..
                }))) if value.value => {
                    quote!(impl #implementation ::veoveo_hub_sdk::calls::TaskToolCall for #name #generics #where_clause {})
                }
                None
                | Some(Argument::Expression(Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Bool(_),
                    ..
                }))) => quote!(),
                Some(value) => {
                    return Err(syn::Error::new_spanned(value, "task must be true or false"));
                }
            };
            quote!(impl #implementation ::veoveo_hub_sdk::calls::ToolCall for #name #generics #where_clause {type Provider=#provider;type Output=#output;const TOOL:&'static str=#tool;} #task)
        }
        "collection_tool" => {
            let collection = args.take("collection")?;
            let tool = args.take("name")?;
            let default=args.0.remove("default_limit").map(|limit|quote!(fn default_limit()-><#collection as ::veoveo_types::Collection>::Limit {<<#collection as ::veoveo_types::Collection>::Limit as ::veoveo_types::PageLimit>::new(#limit).expect("declared default collection limit")}));
            quote!(impl #implementation ::veoveo_hub_sdk::definition::CollectionTool for #name #generics #where_clause {type Provider=#provider;type Collection=#collection;const TOOL:&'static str=#tool;#default})
        }
        "resource_read" => {
            let address_type = args.take("address_type")?;
            let content = args.take("content")?;
            let address = args.take("address")?;
            let template = args.0.remove("template");
            let parser = args.0.remove("parse");
            let member = match (template, parser) {
                (None, None) => quote!(),
                (Some(template), Some(parser)) => {
                    quote!(impl #implementation ::veoveo_hub_sdk::definition::TemplateRead for #name #generics #where_clause {const TEMPLATE:&'static str=#template;fn from_address(address:&Self::Address)->Option<Self>{{let parse:fn(&Self::Address)->Option<Self>=#parser;parse(address)}}})
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        name,
                        "template and parse are supplied together",
                    ));
                }
            };
            quote!(impl #implementation ::veoveo_hub_sdk::calls::ResourceRead for #name #generics #where_clause {type Provider=#provider;type Address=#address_type;type Content=#content;fn address(&self)->Self::Address{#address}} #member)
        }
        "collection_resource" => {
            let collection = args.take("collection")?;
            let address_type = args.take("address_type")?;
            let address = args.take("address")?;
            let filters = args.take("filters")?;
            let template = args.take("template")?;
            let parser = args.take("parse")?;
            let default=args.0.remove("default_limit").map(|limit|quote!(fn default_limit()-><#collection as ::veoveo_types::Collection>::Limit {<<#collection as ::veoveo_types::Collection>::Limit as ::veoveo_types::PageLimit>::new(#limit).expect("declared default collection limit")}));
            quote!(impl #implementation ::veoveo_hub_sdk::definition::CollectionResource for #name #generics #where_clause {
                type Provider=#provider;type Address=#address_type;type Collection=#collection;const TEMPLATE:&'static str=#template;
                fn address(&self)->Self::Address{#address}
                fn filters(&self)-><#collection as ::veoveo_types::Collection>::Filters{#filters}
                fn from_address(address:&Self::Address)->Option<Self>{{let parse:fn(&Self::Address)->Option<Self>=#parser;parse(address)}}
                #default
            })
        }
        _ => unreachable!(),
    };
    args.done()?;
    Ok(quote!(#input #generated))
}
macro_rules! attribute {
    ($name:ident) => {
        #[proc_macro_attribute]
        pub fn $name(attribute: TokenStream, input: TokenStream) -> TokenStream {
            match expand(attribute, input, stringify!($name)) {
                Ok(value) => value.into(),
                Err(error) => error.to_compile_error().into(),
            }
        }
    };
}
attribute!(tool);
attribute!(collection_tool);
attribute!(resource_read);
attribute!(collection_resource);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declaration_types_accept_generic_rust_syntax() {
        let mut arguments: syn::Result<Arguments> = syn::parse_str(
            "provider = Provider<A>, output = Product<Record<B>>, address_type = Address<C>, name = \"produce\"",
        );
        let arguments = arguments.as_mut().unwrap();
        assert!(matches!(
            arguments.take("provider").unwrap(),
            Argument::Type(_)
        ));
        assert!(matches!(
            arguments.take("output").unwrap(),
            Argument::Type(_)
        ));
        assert!(matches!(
            arguments.take("address_type").unwrap(),
            Argument::Type(_)
        ));
        assert!(matches!(
            arguments.take("name").unwrap(),
            Argument::Expression(_)
        ));
    }
    #[test]
    fn task_mode_requires_a_boolean_literal() {
        assert!(syn::parse_str::<Arguments>("task = 1").is_err());
        assert!(syn::parse_str::<Arguments>("task = \"true\"").is_err());
        assert!(syn::parse_str::<Arguments>("task = true").is_ok());
    }
}
