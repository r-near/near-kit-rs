//! Proc macros for near-kit typed contract interfaces.
//!
//! Use the macro through near-kit's `contracts` feature (`#[near_kit::contract]`)
//! or near-kit-primitives' `contracts` feature, not by depending on this crate
//! directly. The `#[call]`, `#[json]` and `#[borsh]` markers inside the trait
//! are read by `#[contract]` itself and need no import.
//!
//! # Example
//!
//! ```rust,no_run
//! use near_kit::protocol::Action;
//! use near_kit::*;
//!
//! #[near_kit::contract]
//! pub trait Counter {
//!     fn get_count(&self) -> u64;
//!
//!     #[call]
//!     fn add(&mut self, args: AddArgs);
//! }
//!
//! #[derive(serde::Serialize)]
//! pub struct AddArgs {
//!     pub value: u64,
//! }
//!
//! # async fn example(near: &Near, alice: &SigningNear) -> Result<(), Error> {
//! // A client: views on any `Viewer`, calls on any `TxSender`.
//! let count = near.contract::<Counter>(account!("counter.near")).get_count().await?;
//! alice
//!     .contract::<Counter>(account!("counter.near"))
//!     .add(AddArgs { value: 5 })
//!     .gas(Gas::from_tgas(10))
//!     .await?;
//!
//! // An offline `FunctionCall`, to batch with other actions.
//! alice
//!     .transaction(account!("counter.near"))
//!     .add_action(Action::transfer(NearToken::from_millinear(1)))
//!     .add_action(Counter::add(AddArgs { value: 1 }))
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Per-method format
//!
//! Arguments and results are JSON by default. `#[near_kit::contract(borsh)]`
//! switches the default; `#[borsh]` or `#[json]` overrides it for one method:
//!
//! ```rust
//! #[near_kit::contract]
//! pub trait Mixed {
//!     fn get_json(&self) -> u64;
//!
//!     #[borsh]
//!     fn get_borsh(&self) -> u64;
//!
//!     #[call]
//!     #[borsh]
//!     fn set_borsh(&mut self, value: u64);
//! }
//! ```

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    FnArg, Ident, ItemTrait, Pat, ReceiverKind, ReturnType, TraitItem, TraitItemFn, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
};

/// Serialization format for contract methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SerializationFormat {
    #[default]
    Json,
    Borsh,
}

/// The crate the expansion names for its runtime support.
///
/// near-kit when the caller depends on it, so the generated client can
/// default its backend to `near_kit::Near`. Otherwise near-kit-primitives,
/// which has every item the expansion names, so crates that can't take the
/// RPC client, such as contracts built for wasm, can still use the macro.
fn near_kit_path() -> TokenStream2 {
    let found = |name: &str, itself: TokenStream2| match crate_name(name) {
        Ok(FoundCrate::Itself) => Some(itself),
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{name}");
            Some(quote!(::#ident))
        }
        Err(_) => None,
    };
    // near-kit declares `extern crate self as near_kit`, so `::near_kit` also
    // resolves inside near-kit and in its own examples, tests and doctests
    // (which are separate crates). The same goes for near-kit-primitives.
    found("near-kit", quote!(::near_kit))
        .or_else(|| found("near-kit-primitives", quote!(::near_kit_primitives)))
        .unwrap_or_else(|| quote!(::near_kit))
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
                ));
            }
        };

        Ok(Self { format })
    }
}

/// Information about a parsed method.
#[derive(Debug)]
struct MethodInfo {
    name: Ident,
    /// The method's `#[doc]` attributes, copied onto the generated methods.
    docs: Vec<syn::Attribute>,
    is_view: bool,
    /// Per-method format override (if specified via #[json] or #[borsh])
    format_override: Option<SerializationFormat>,
    arg_name: Option<Ident>,
    arg_type: Option<Type>,
    return_type: Option<Type>,
}

/// Parse a method from a trait item.
fn parse_method(method: &TraitItemFn) -> syn::Result<MethodInfo> {
    let name = method.sig.ident.clone();

    // `contract_id` is the generated client's accessor and `client` is the
    // `Contract` constructor on the marker type; a contract method with
    // either name would collide with or shadow them.
    if name == "contract_id" || name == "client" {
        return Err(syn::Error::new(
            name.span(),
            format!(
                "`{name}` is reserved by #[near_kit::contract] (the generated client uses it); \
                 rename the method"
            ),
        ));
    }

    // Check receiver type
    let receiver = method.sig.receiver();
    let (is_view, is_mut) = match receiver {
        // Note: for reference receivers, syn stores the `mut` inside
        // `ReceiverKind::Reference` rather than in `Receiver::mutability`.
        Some(recv) => match &recv.kind {
            ReceiverKind::Reference(_, _, mutability) => {
                (mutability.is_none(), mutability.is_some())
            }
            _ => {
                return Err(syn::Error::new(
                    recv.span(),
                    "contract methods must take &self or &mut self",
                ));
            }
        },
        None => {
            return Err(syn::Error::new(
                method.sig.span(),
                "contract methods must have a receiver (&self or &mut self)",
            ));
        }
    };

    // Check for #[call] attribute
    let mut is_call = false;
    for attr in method
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("call"))
    {
        is_call = true;
        if attr.meta.require_path_only().is_err() {
            return Err(syn::Error::new_spanned(
                &attr.meta,
                "#[call] does not accept options",
            ));
        }
    }

    // Check for a #[json] or #[borsh] format override.
    let mut format_override = None;
    for attr in method
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("json") || attr.path().is_ident("borsh"))
    {
        let (format, name) = if attr.path().is_ident("json") {
            (SerializationFormat::Json, "json")
        } else {
            (SerializationFormat::Borsh, "borsh")
        };

        if attr.meta.require_path_only().is_err() {
            return Err(syn::Error::new_spanned(
                &attr.meta,
                format!("#[{name}] does not accept options"),
            ));
        }

        match format_override {
            Some(previous) if previous == format => {
                return Err(syn::Error::new_spanned(
                    &attr.meta,
                    format!("#[{name}] may only be specified once"),
                ));
            }
            Some(_) => {
                return Err(syn::Error::new_spanned(
                    &attr.meta,
                    "#[json] and #[borsh] cannot be used together",
                ));
            }
            None => format_override = Some(format),
        }
    }

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

    // Parse return type. A `#[call]` resolves to the sender's
    // `TxSender::Output`, so a declared return type would be silently
    // ignored; reject it so it can be given a meaning later.
    let return_type = match &method.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) if is_call => {
            return Err(syn::Error::new(
                ty.span(),
                "`#[call]` methods resolve to the sender's `TxSender::Output`; \
                 remove the return type",
            ));
        }
        ReturnType::Type(_, ty) => Some((**ty).clone()),
    };

    let docs = method
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .cloned()
        .collect();

    Ok(MethodInfo {
        name,
        docs,
        is_view,
        format_override,
        arg_name,
        arg_type,
        return_type,
    })
}

/// Generate the client method for a view function.
///
/// The `where C: Viewer + Clone` bound sits on the method, not the impl
/// block, so calling it on a client whose backend can't view reports the
/// trait's `on_unimplemented` text (E0277) instead of "method not found"
/// (E0599). `Clone` comes after the trait so that error is reported first.
/// The view gets its own clone of the backend, so it can outlive `&self`.
fn generate_view_method(method: &MethodInfo, contract_format: SerializationFormat) -> TokenStream2 {
    let near_kit = near_kit_path();
    let method_name = &method.name;
    let method_name_str = method_name.to_string();
    let docs = &method.docs;

    let format = method.format_override.unwrap_or(contract_format);

    let return_type = method
        .return_type
        .as_ref()
        .map(|t| quote! { #t })
        .unwrap_or_else(|| quote! { () });

    let constructor = match format {
        SerializationFormat::Json => quote! { #near_kit::__private::view_json },
        SerializationFormat::Borsh => quote! { #near_kit::__private::view_borsh },
    };

    // Arguments are encoded now, so borrowed argument types never enter the
    // future. Without arguments, JSON views send `{}` and Borsh views nothing.
    let (params, args) = match (&method.arg_name, &method.arg_type, format) {
        (Some(arg_name), Some(arg_type), SerializationFormat::Json) => (
            quote! { #arg_name: #arg_type },
            quote! { #near_kit::__private::json_args(&#arg_name) },
        ),
        (Some(arg_name), Some(arg_type), SerializationFormat::Borsh) => (
            quote! { #arg_name: #arg_type },
            quote! { #near_kit::__private::borsh_args(&#arg_name) },
        ),
        (_, _, SerializationFormat::Json) => (quote! {}, quote! { Ok(b"{}".to_vec()) }),
        (_, _, SerializationFormat::Borsh) => (quote! {}, quote! { Ok(::std::vec::Vec::new()) }),
    };

    quote! {
        #(#docs)*
        pub fn #method_name(&self, #params) -> #near_kit::transaction::View<C, #return_type>
        where
            C: #near_kit::Viewer + ::core::clone::Clone,
        {
            #constructor(
                ::core::clone::Clone::clone(&self.backend),
                self.contract_id.clone(),
                #method_name_str,
                #args,
            )
        }
    }
}

/// Generate the client method for a call function, on top of the static
/// `FunctionCall` constructor. Bounded at the method for the same reason as
/// views.
fn generate_call_method(method: &MethodInfo, trait_name: &Ident) -> TokenStream2 {
    let near_kit = near_kit_path();
    let method_name = &method.name;
    let docs = &method.docs;

    let (params, args) = match (&method.arg_name, &method.arg_type) {
        (Some(arg_name), Some(arg_type)) => (quote! { #arg_name: #arg_type }, quote! { #arg_name }),
        _ => (quote! {}, quote! {}),
    };

    quote! {
        #(#docs)*
        pub fn #method_name(&self, #params) -> #near_kit::transaction::Call<C>
        where
            C: #near_kit::TxSender + ::core::clone::Clone,
        {
            #near_kit::__private::call(
                ::core::clone::Clone::clone(&self.backend),
                self.contract_id.clone(),
                #trait_name::#method_name(#args),
            )
        }
    }
}

/// Generate a static associated function on the contract struct that returns `FunctionCall`.
///
/// Only generated for call methods (not view methods). This enables composable
/// transactions where typed contract calls can be mixed with other actions.
fn generate_function_call_method(
    method: &MethodInfo,
    contract_format: SerializationFormat,
) -> TokenStream2 {
    let near_kit = near_kit_path();
    let method_name = &method.name;
    let method_name_str = method_name.to_string();

    let format = method.format_override.unwrap_or(contract_format);
    let docs = &method.docs;

    if let (Some(arg_name), Some(arg_type)) = (&method.arg_name, &method.arg_type) {
        let args_method = match format {
            SerializationFormat::Json => quote! { .args(#arg_name) },
            SerializationFormat::Borsh => quote! { .args_borsh(#arg_name) },
        };

        quote! {
            #(#docs)*
            pub fn #method_name(#arg_name: #arg_type) -> #near_kit::transaction::FunctionCall {
                #near_kit::transaction::FunctionCall::new(#method_name_str)
                    #args_method
            }
        }
    } else {
        match format {
            SerializationFormat::Json => {
                // Use args_raw to avoid depending on serde_json in expanded code
                quote! {
                    #(#docs)*
                    pub fn #method_name() -> #near_kit::transaction::FunctionCall {
                        #near_kit::transaction::FunctionCall::new(#method_name_str)
                            .args_raw(b"{}".to_vec())
                    }
                }
            }
            SerializationFormat::Borsh => {
                quote! {
                    #(#docs)*
                    pub fn #method_name() -> #near_kit::transaction::FunctionCall {
                        #near_kit::transaction::FunctionCall::new(#method_name_str)
                    }
                }
            }
        }
    }
}

/// Define a typed contract interface from a trait.
///
/// For `trait Counter`, this generates:
///
/// - a unit struct `Counter` with a static `FunctionCall` constructor for each
///   `#[call]` method (e.g. `Counter::increment()`), for composing typed calls
///   into transactions;
/// - a `CounterClient<C>` generic over its backend `C`: view methods need
///   `C: Viewer + Clone` and return a `View`, `#[call]` methods need
///   `C: TxSender + Clone` and return a `Call`; each owns a clone of the
///   backend. When the expansion resolves to near-kit with its `rpc`
///   feature, `C` defaults to `near_kit::Near`;
/// - `impl Contract for Counter`, which `Near::contract::<Counter>(id)` uses
///   to build the client.
///
/// Everything it names is in near-kit-primitives, so it works without the
/// RPC client (e.g. in contracts built for wasm).
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
    let near_kit = near_kit_path();
    let trait_name = &input.ident;
    let client_name = format_ident!("{}Client", trait_name);
    let vis = &input.vis;

    // Reject unsupported trait features
    if let Some(unsafety) = input.unsafety {
        return Err(syn::Error::new(
            unsafety.span(),
            "#[near_kit::contract] does not support unsafe traits",
        ));
    }
    if let Some(auto_token) = input.modifiers.auto_token {
        return Err(syn::Error::new(
            auto_token.span(),
            "#[near_kit::contract] does not support auto traits",
        ));
    }
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "#[near_kit::contract] does not support generic parameters",
        ));
    }
    if let Some(where_clause) = &input.generics.where_clause {
        return Err(syn::Error::new(
            where_clause.span(),
            "#[near_kit::contract] does not support where clauses",
        ));
    }
    if !input.supertraits.is_empty() {
        return Err(syn::Error::new(
            input.supertraits.span(),
            "#[near_kit::contract] does not support supertraits",
        ));
    }

    // Parse all methods, reject non-method items
    let mut methods = Vec::new();
    for item in &input.items {
        match item {
            TraitItem::Fn(method) => {
                if method.default.is_some() {
                    return Err(syn::Error::new(
                        method.sig.span(),
                        "#[near_kit::contract] does not support default method implementations",
                    ));
                }
                methods.push(parse_method(method)?);
            }
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "#[near_kit::contract] only supports methods in traits",
                ));
            }
        }
    }

    // Client methods: views return `View`, calls return `Call`.
    let client_methods: Vec<TokenStream2> = methods
        .iter()
        .map(|m| {
            if m.is_view {
                generate_view_method(m, args.format)
            } else {
                generate_call_method(m, trait_name)
            }
        })
        .collect();

    // Generate FunctionCall constructors for call methods only
    let function_call_methods: Vec<TokenStream2> = methods
        .iter()
        .filter(|m| !m.is_view)
        .map(|m| generate_function_call_method(m, args.format))
        .collect();

    // Propagate trait-level attributes (doc comments, #[cfg], etc.) to the struct
    let trait_attrs = &input.attrs;

    let client_doc = format!(
        " Client for the [`{trait_name}`] contract, sending through the backend `C`.\n\n \
         View methods need `C: Viewer + Clone`; `#[call]` methods need `C: TxSender + Clone`."
    );

    let expanded = quote! {
        #(#trait_attrs)*
        #[derive(Debug, Clone)]
        #vis struct #trait_name;

        impl #trait_name {
            #(#function_call_methods)*
        }

        // `client_struct!` adds the `= Near` default for `C` when near-kit
        // has its client; a `cfg` here would test the caller's features.
        #near_kit::__private::client_struct! {
            #[doc = #client_doc]
            #[derive(Debug, Clone)]
            #vis struct #client_name<C> {
                backend: C,
                contract_id: #near_kit::AccountId,
            }
        }

        impl<C> #client_name<C> {
            /// The contract account this client talks to.
            pub fn contract_id(&self) -> &#near_kit::AccountId {
                &self.contract_id
            }

            #(#client_methods)*
        }

        impl #near_kit::Contract for #trait_name {
            type Client<C> = #client_name<C>;

            fn client<C>(backend: C, contract_id: #near_kit::AccountId) -> #client_name<C> {
                #client_name {
                    backend,
                    contract_id,
                }
            }
        }
    };

    Ok(expanded)
}
