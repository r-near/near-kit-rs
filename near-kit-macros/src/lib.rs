//! Proc macros for near-kit typed contract interfaces.
//!
//! This crate provides the `#[near_kit::contract]` attribute macro for defining
//! type-safe contract interfaces.
//!
//! # Example
//!
//! ```ignore
//! use near_kit::prelude::*;
//! use serde::Serialize;
//!
//! #[near_kit::contract]
//! pub trait Counter {
//!     fn get_count(&self) -> u64;
//!     
//!     #[call]
//!     fn increment(&mut self);
//!     
//!     #[call]
//!     fn add(&mut self, args: AddArgs);
//! }
//!
//! #[derive(Serialize)]
//! pub struct AddArgs {
//!     pub value: u64,
//! }
//! ```

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    FnArg, Ident, ItemTrait, Pat, ReturnType, TraitItem, TraitItemFn, Type,
};

/// Serialization format for contract methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SerializationFormat {
    #[default]
    Json,
    Borsh,
}

/// Arguments to the `#[contract]` attribute.
#[derive(Debug, Default)]
struct ContractArgs {
    format: SerializationFormat,
}

impl Parse for ContractArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(Self::default());
        }

        let ident: Ident = input.parse()?;
        let format = match ident.to_string().as_str() {
            "json" => SerializationFormat::Json,
            "borsh" => SerializationFormat::Borsh,
            other => {
                return Err(syn::Error::new(
                    ident.span(),
                    format!("unknown format '{}', expected 'json' or 'borsh'", other),
                ))
            }
        };

        Ok(Self { format })
    }
}

/// Arguments to the `#[call]` attribute.
#[derive(Debug, Default)]
struct CallArgs {
    payable: bool,
}

impl Parse for CallArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(Self::default());
        }

        let ident: Ident = input.parse()?;
        if ident != "payable" {
            return Err(syn::Error::new(
                ident.span(),
                format!("unknown call option '{}', expected 'payable'", ident),
            ));
        }

        Ok(Self { payable: true })
    }
}

/// Information about a parsed method.
#[derive(Debug)]
struct MethodInfo {
    name: Ident,
    is_view: bool,
    #[allow(dead_code)] // Reserved for future validation
    is_call: bool,
    #[allow(dead_code)] // Reserved for payable method handling
    is_payable: bool,
    arg_name: Option<Ident>,
    arg_type: Option<Type>,
    return_type: Option<Type>,
}

/// Parse a method from a trait item.
fn parse_method(method: &TraitItemFn) -> syn::Result<MethodInfo> {
    let name = method.sig.ident.clone();

    // Check receiver type
    let receiver = method.sig.receiver();
    let (is_view, is_mut) = match receiver {
        Some(recv) => {
            if recv.reference.is_some() {
                (recv.mutability.is_none(), recv.mutability.is_some())
            } else {
                return Err(syn::Error::new(
                    recv.span(),
                    "contract methods must take &self or &mut self",
                ));
            }
        }
        None => {
            return Err(syn::Error::new(
                method.sig.span(),
                "contract methods must have a receiver (&self or &mut self)",
            ))
        }
    };

    // Check for #[call] attribute
    let call_attr = method
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("call"));

    let (is_call, is_payable) = match call_attr {
        Some(attr) => {
            let args: CallArgs = if attr.meta.require_path_only().is_ok() {
                CallArgs::default()
            } else {
                attr.parse_args()?
            };
            (true, args.payable)
        }
        None => (false, false),
    };

    // Validate: view methods should not have #[call]
    if is_view && is_call {
        return Err(syn::Error::new(
            method.sig.span(),
            "view methods (&self) should not have #[call] attribute",
        ));
    }

    // Validate: call methods must have #[call]
    if is_mut && !is_call {
        return Err(syn::Error::new(
            method.sig.span(),
            "call methods (&mut self) must have #[call] attribute",
        ));
    }

    // Parse arguments (excluding self)
    let mut arg_name = None;
    let mut arg_type = None;
    let mut arg_count = 0;

    for arg in &method.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            arg_count += 1;
            if arg_count > 1 {
                return Err(syn::Error::new(
                    pat_type.span(),
                    "contract methods can have at most one argument (use a struct for multiple parameters)",
                ));
            }

            // Extract argument name
            if let Pat::Ident(pat_ident) = pat_type.pat.as_ref() {
                arg_name = Some(pat_ident.ident.clone());
            }
            arg_type = Some((*pat_type.ty).clone());
        }
    }

    // Parse return type
    let return_type = match &method.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => Some((**ty).clone()),
    };

    Ok(MethodInfo {
        name,
        is_view,
        is_call,
        is_payable,
        arg_name,
        arg_type,
        return_type,
    })
}

/// Generate client method for a view function.
fn generate_view_method(method: &MethodInfo, format: SerializationFormat) -> TokenStream2 {
    let method_name = &method.name;
    let method_name_str = method_name.to_string();

    let return_type = method
        .return_type
        .as_ref()
        .map(|t| quote! { #t })
        .unwrap_or_else(|| quote! { () });

    // Determine which view method to use based on format
    let view_call = match format {
        SerializationFormat::Json => quote! { self.near.view },
        SerializationFormat::Borsh => quote! { self.near.view_borsh },
    };

    if let (Some(arg_name), Some(arg_type)) = (&method.arg_name, &method.arg_type) {
        // View with args
        let args_method = match format {
            SerializationFormat::Json => quote! { .args(#arg_name) },
            SerializationFormat::Borsh => quote! { .args_borsh(#arg_name) },
        };

        quote! {
            pub fn #method_name(&self, #arg_name: #arg_type) -> near_kit::ViewCall<#return_type> {
                #view_call::<#return_type>(&self.contract_id, #method_name_str)
                    #args_method
            }
        }
    } else {
        // View without args - for JSON, pass empty object; for Borsh, no args
        match format {
            SerializationFormat::Json => {
                quote! {
                    pub fn #method_name(&self) -> near_kit::ViewCall<#return_type> {
                        #view_call::<#return_type>(&self.contract_id, #method_name_str)
                            .args(serde_json::json!({}))
                    }
                }
            }
            SerializationFormat::Borsh => {
                quote! {
                    pub fn #method_name(&self) -> near_kit::ViewCall<#return_type> {
                        #view_call::<#return_type>(&self.contract_id, #method_name_str)
                    }
                }
            }
        }
    }
}

/// Generate client method for a call function.
fn generate_call_method(method: &MethodInfo, format: SerializationFormat) -> TokenStream2 {
    let method_name = &method.name;
    let method_name_str = method_name.to_string();

    if let (Some(arg_name), Some(arg_type)) = (&method.arg_name, &method.arg_type) {
        // Call with args
        let args_method = match format {
            SerializationFormat::Json => quote! { .args(#arg_name) },
            SerializationFormat::Borsh => quote! { .args_borsh(#arg_name) },
        };

        quote! {
            pub fn #method_name(&self, #arg_name: #arg_type) -> near_kit::ContractCall {
                self.near.call(&self.contract_id, #method_name_str)
                    #args_method
            }
        }
    } else {
        // Call without args - for JSON, pass empty object; for Borsh, no args
        match format {
            SerializationFormat::Json => {
                quote! {
                    pub fn #method_name(&self) -> near_kit::ContractCall {
                        self.near.call(&self.contract_id, #method_name_str)
                            .args(serde_json::json!({}))
                    }
                }
            }
            SerializationFormat::Borsh => {
                quote! {
                    pub fn #method_name(&self) -> near_kit::ContractCall {
                        self.near.call(&self.contract_id, #method_name_str)
                    }
                }
            }
        }
    }
}

/// Strip the #[call] attribute from a method for the output trait.
fn strip_call_attr(method: &TraitItemFn) -> TraitItemFn {
    let mut method = method.clone();
    method.attrs.retain(|attr| !attr.path().is_ident("call"));
    method
}

/// The main contract macro implementation.
#[proc_macro_attribute]
pub fn contract(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ContractArgs);
    let input = parse_macro_input!(item as ItemTrait);

    match contract_impl(args, input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn contract_impl(args: ContractArgs, input: ItemTrait) -> syn::Result<TokenStream2> {
    let trait_name = &input.ident;
    let client_name = format_ident!("{}Client", trait_name);
    let vis = &input.vis;

    // Parse all methods
    let mut methods = Vec::new();
    for item in &input.items {
        if let TraitItem::Fn(method) = item {
            methods.push(parse_method(method)?);
        }
    }

    // Generate client methods
    let client_methods: Vec<TokenStream2> = methods
        .iter()
        .map(|m| {
            if m.is_view {
                generate_view_method(m, args.format)
            } else {
                generate_call_method(m, args.format)
            }
        })
        .collect();

    // Generate the cleaned trait (without #[call] attributes)
    let cleaned_items: Vec<TraitItem> = input
        .items
        .iter()
        .map(|item| {
            if let TraitItem::Fn(method) = item {
                TraitItem::Fn(strip_call_attr(method))
            } else {
                item.clone()
            }
        })
        .collect();

    let trait_attrs = &input.attrs;
    let trait_supertraits = &input.supertraits;
    let trait_generics = &input.generics;

    // Build the output
    let expanded = quote! {
        // Original trait (with #[call] attrs stripped for cleaner output)
        #(#trait_attrs)*
        #vis trait #trait_name #trait_generics : #trait_supertraits {
            #(#cleaned_items)*
        }

        // Generated client struct
        #vis struct #client_name<'a> {
            near: &'a near_kit::Near,
            contract_id: near_kit::AccountId,
        }

        impl<'a> #client_name<'a> {
            /// Create a new contract client.
            pub fn new(near: &'a near_kit::Near, contract_id: near_kit::AccountId) -> Self {
                Self { near, contract_id }
            }

            /// Get the contract account ID.
            pub fn contract_id(&self) -> &near_kit::AccountId {
                &self.contract_id
            }

            #(#client_methods)*
        }

        // Implement ContractClient trait for construction via near.contract::<T>()
        impl<'a> near_kit::contract::ContractClient<'a> for #client_name<'a> {
            fn new(near: &'a near_kit::Near, contract_id: near_kit::AccountId) -> Self {
                Self { near, contract_id }
            }
        }

        // Implement Contract marker trait
        impl near_kit::Contract for dyn #trait_name {
            type Client<'a> = #client_name<'a>;
        }
    };

    Ok(expanded)
}

/// Attribute macro for marking call methods.
///
/// This is used internally by `#[near_kit::contract]` traits.
///
/// # Examples
///
/// ```ignore
/// #[call]
/// fn increment(&mut self);
///
/// #[call(payable)]
/// fn donate(&mut self);
/// ```
#[proc_macro_attribute]
pub fn call(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // This is just a marker attribute - the actual work is done by #[contract]
    item
}
