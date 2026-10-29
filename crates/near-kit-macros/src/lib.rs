//! The `#[contract]` macro, which turns a trait describing a NEAR contract
//! into a typed client.
//!
//! Don't depend on this crate directly. Enable near-kit's `contracts`
//! feature and write `#[near_kit::contract]`, or near-kit-primitives'
//! `contracts` feature (`#[near_kit_primitives::contract]`) where the RPC
//! client can't go, such as inside a contract. The `#[call]`, `#[json]` and
//! `#[borsh]` markers inside the trait are read by `#[contract]` itself and
//! need no import.
//!
//! [`contract`] lists what the macro generates and the rules for methods.
//! near-kit's
//! [contracts and tokens guide](https://docs.rs/near-kit/0.30.0-rc.1/near_kit/guide/contracts_and_tokens/index.html)
//! shows it in use with every kind of backend.

#![warn(missing_docs)]

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    FnArg, Ident, ItemTrait, Pat, ReceiverKind, ReturnType, TraitItem, TraitItemFn, Type,
    ext::IdentExt,
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    spanned::Spanned,
};

#[derive(Clone, Copy, PartialEq, Default)]
enum SerializationFormat {
    #[default]
    Json,
    Borsh,
}

/// The generated client's backend type parameter. Spelled so it can't
/// shadow a type the contract's methods name (a user type `C`, say).
fn backend_param() -> Ident {
    format_ident!("__NearKitBackend")
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

/// The `#[contract]` attribute's argument: the contract's default format.
impl Parse for SerializationFormat {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(Self::default());
        }
        let ident: Ident = input.parse()?;
        match ident.to_string().as_str() {
            "json" => Ok(Self::Json),
            "borsh" => Ok(Self::Borsh),
            other => Err(syn::Error::new(
                ident.span(),
                format!("unknown format '{other}', expected 'json' or 'borsh'"),
            )),
        }
    }
}

struct MethodInfo {
    name: Ident,
    /// The method's `#[doc]` attributes, copied onto the generated methods.
    docs: Vec<syn::Attribute>,
    /// The method's `#[cfg]` and `#[cfg_attr]` attributes, copied onto every
    /// item generated for it.
    cfgs: Vec<syn::Attribute>,
    is_view: bool,
    /// Set by `#[json]` or `#[borsh]`.
    format_override: Option<SerializationFormat>,
    arg: Option<(Ident, Type)>,
    return_type: Type,
}

fn parse_method(method: &TraitItemFn) -> syn::Result<MethodInfo> {
    let name = method.sig.ident.clone();
    let unraw_name = name.unraw().to_string();

    // `contract_id` is the generated client's accessor and `client` is the
    // `Contract` constructor on the marker type; a contract method with
    // either name would collide with or shadow them.
    if unraw_name == "contract_id" || unraw_name == "client" {
        return Err(syn::Error::new(
            name.span(),
            format!(
                "`{unraw_name}` is reserved by #[near_kit::contract] (the generated client uses \
                 it); rename the method"
            ),
        ));
    }
    // An inherent `clone` on the client would shadow its `Clone::clone`.
    if unraw_name == "clone" {
        return Err(syn::Error::new(
            name.span(),
            "`clone` is reserved by #[near_kit::contract] (it would shadow `Clone::clone` on \
             the generated client); rename the method",
        ));
    }

    // The generated methods return `View`/`Call`, which are awaited; they
    // can't be `async` themselves.
    if let Some(asyncness) = &method.sig.asyncness {
        return Err(syn::Error::new(
            asyncness.span(),
            "contract methods can't be `async`; the generated method returns a `View` or \
             `Call` to `.await`, so remove `async`",
        ));
    }

    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            "#[near_kit::contract] does not support generic methods or lifetime parameters \
             (use `impl Trait` or a concrete argument type)",
        ));
    }
    if let Some(where_clause) = &method.sig.generics.where_clause {
        return Err(syn::Error::new_spanned(
            where_clause,
            "#[near_kit::contract] does not support where clauses on methods",
        ));
    }

    // `#[call]`, `#[json]` and `#[borsh]` are read here, before any `cfg` is
    // evaluated, so one behind `#[cfg_attr]` would be ignored for the method
    // and then copied onto the generated items, where it is no attribute at
    // all. Reject it instead.
    for attr in &method.attrs {
        if let syn::Meta::List(list) = &attr.meta
            && list.path.is_ident("cfg_attr")
            && let Some(marker) = conditional_marker(list)
        {
            let name = marker
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            return Err(syn::Error::new_spanned(
                marker,
                format!(
                    "#[{name}] can't be applied through #[cfg_attr]: #[near_kit::contract] reads \
                     it before `cfg` is evaluated. Declare the method twice instead, each copy \
                     under its own #[cfg]"
                ),
            ));
        }
    }

    let is_view = match method.sig.receiver() {
        Some(recv) => match &recv.kind {
            ReceiverKind::Reference(_, _, mutability) => mutability.is_none(),
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

    if is_view && is_call {
        return Err(syn::Error::new(
            method.sig.span(),
            "view methods (&self) should not have #[call] attribute",
        ));
    }

    if !is_view && !is_call {
        return Err(syn::Error::new(
            method.sig.span(),
            "call methods (&mut self) must have #[call] attribute",
        ));
    }

    let mut typed = method.sig.inputs.iter().filter_map(|arg| match arg {
        FnArg::Typed(pat_type) => Some(pat_type),
        FnArg::Receiver(_) => None,
    });
    // The name becomes the generated method's parameter, so it must be a
    // plain identifier (`args`, `mut args`).
    let arg = match typed.next() {
        None => None,
        Some(pat_type) => match pat_type.pat.as_ref() {
            Pat::Ident(pat_ident) if pat_ident.by_ref.is_none() && pat_ident.subpat.is_none() => {
                Some((pat_ident.ident.clone(), (*pat_type.ty).clone()))
            }
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "contract method arguments must be a plain name, like `args: Args` \
                     (the generated method takes the argument by that name)",
                ));
            }
        },
    };
    if let Some(second) = typed.next() {
        return Err(syn::Error::new(
            second.span(),
            "contract methods can have at most one argument (use a struct for multiple parameters)",
        ));
    }

    // A `#[call]` resolves to the sender's `TxSender::Output`, so a declared
    // return type would be silently ignored; reject it so it can be given a
    // meaning later.
    let return_type = match &method.sig.output {
        ReturnType::Default => parse_quote!(()),
        ReturnType::Type(_, ty) if is_call => {
            return Err(syn::Error::new(
                ty.span(),
                "`#[call]` methods resolve to the sender's `TxSender::Output`; \
                 remove the return type",
            ));
        }
        ReturnType::Type(_, ty) => (**ty).clone(),
    };

    let docs = method
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .cloned()
        .collect();
    let cfgs = method
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
        .cloned()
        .collect();

    Ok(MethodInfo {
        name,
        docs,
        cfgs,
        is_view,
        format_override,
        arg,
        return_type,
    })
}

/// The first `#[contract]` marker (`call`, `json`, `borsh`) that a
/// `cfg_attr(predicate, attrs..)` would apply, looking inside nested
/// `cfg_attr`s too. Contents that don't parse are left for rustc to report.
fn conditional_marker(cfg_attr: &syn::MetaList) -> Option<syn::Path> {
    let metas = cfg_attr
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .ok()?;
    // The first entry is the predicate.
    metas.into_iter().skip(1).find_map(|meta| match meta {
        syn::Meta::List(nested) if nested.path.is_ident("cfg_attr") => conditional_marker(&nested),
        meta if ["call", "json", "borsh"]
            .iter()
            .any(|marker| meta.path().is_ident(marker)) =>
        {
            Some(meta.path().clone())
        }
        _ => None,
    })
}

/// Generate the client method for a view function.
///
/// The `Viewer + Clone` bound on the backend sits on the method, not the impl
/// block, so calling it on a client whose backend can't view reports the
/// trait's `on_unimplemented` text (E0277) instead of "method not found"
/// (E0599). `Clone` comes after the trait so that error is reported first.
/// The view gets its own clone of the backend, so it can outlive `&self`.
fn generate_view_method(
    method: &MethodInfo,
    contract_format: SerializationFormat,
    near_kit: &TokenStream2,
    backend: &Ident,
) -> TokenStream2 {
    let method_name = &method.name;
    // `fn r#type` is the contract method `type`.
    let method_name_str = method_name.unraw().to_string();
    let docs = &method.docs;
    let cfgs = &method.cfgs;
    let return_type = &method.return_type;
    let format = method.format_override.unwrap_or(contract_format);

    let constructor = match format {
        SerializationFormat::Json => quote! { #near_kit::__private::view_json },
        SerializationFormat::Borsh => quote! { #near_kit::__private::view_borsh },
    };

    // Arguments are encoded now, so borrowed argument types never enter the
    // future. Without arguments, JSON views send `{}` and Borsh views nothing.
    let (params, args) = match (&method.arg, format) {
        (Some((arg_name, arg_type)), SerializationFormat::Json) => (
            quote! { #arg_name: #arg_type },
            quote! { #near_kit::__private::json_args(&#arg_name) },
        ),
        (Some((arg_name, arg_type)), SerializationFormat::Borsh) => (
            quote! { #arg_name: #arg_type },
            quote! { #near_kit::__private::borsh_args(&#arg_name) },
        ),
        (None, SerializationFormat::Json) => (quote! {}, quote! { Ok(b"{}".to_vec()) }),
        (None, SerializationFormat::Borsh) => (quote! {}, quote! { Ok(::std::vec::Vec::new()) }),
    };

    quote! {
        #(#cfgs)*
        #(#docs)*
        pub fn #method_name(&self, #params) -> #near_kit::View<#backend, #return_type>
        where
            #backend: #near_kit::Viewer + ::core::clone::Clone,
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

/// The `name: Type` parameter and the bare `name` for a method's argument,
/// or nothing for a method without one.
fn arg_tokens(method: &MethodInfo) -> (Option<TokenStream2>, Option<&Ident>) {
    method
        .arg
        .as_ref()
        .map(|(name, ty)| (quote! { #name: #ty }, name))
        .unzip()
}

/// Generate the client method for a call function, on top of the static
/// `FunctionCall` constructor. Bounded at the method for the same reason as
/// views.
fn generate_call_method(
    method: &MethodInfo,
    trait_name: &Ident,
    near_kit: &TokenStream2,
    backend: &Ident,
) -> TokenStream2 {
    let method_name = &method.name;
    let docs = &method.docs;
    let cfgs = &method.cfgs;
    let (params, args) = arg_tokens(method);

    quote! {
        #(#cfgs)*
        #(#docs)*
        pub fn #method_name(&self, #params) -> #near_kit::Call<#backend>
        where
            #backend: #near_kit::TxSender + ::core::clone::Clone,
        {
            // The constructor carries the method's attributes, so a
            // `#[deprecated]` method would warn here, in generated code.
            #[allow(deprecated)]
            let function_call = #trait_name::#method_name(#args);
            #near_kit::__private::call(
                ::core::clone::Clone::clone(&self.backend),
                self.contract_id.clone(),
                function_call,
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
    near_kit: &TokenStream2,
) -> TokenStream2 {
    let method_name = &method.name;
    let method_name_str = method_name.unraw().to_string();
    let docs = &method.docs;
    let cfgs = &method.cfgs;
    let (params, arg) = arg_tokens(method);

    let args_call = match (arg, method.format_override.unwrap_or(contract_format)) {
        (Some(arg), SerializationFormat::Json) => quote! { .args(#arg) },
        (Some(arg), SerializationFormat::Borsh) => quote! { .args_borsh(#arg) },
        (None, SerializationFormat::Json) => quote! { .args_raw(b"{}".to_vec()) },
        (None, SerializationFormat::Borsh) => quote! {},
    };

    quote! {
        #(#cfgs)*
        #(#docs)*
        pub fn #method_name(#params) -> #near_kit::transaction::FunctionCall {
            #near_kit::transaction::FunctionCall::new(#method_name_str) #args_call
        }
    }
}

/// Define a typed contract interface from a trait.
///
/// For `trait Counter`, this generates:
///
/// - a unit struct `Counter` with a static `FunctionCall` constructor for each
///   `#[call]` method (e.g. `Counter::add(args)`), for composing typed calls
///   into transactions;
/// - a `CounterClient<C>` generic over its backend `C`: view methods need
///   `C: Viewer + Clone` and return a `View`, `#[call]` methods need
///   `C: TxSender + Clone` and return a `Call`; each owns a clone of the
///   backend (`&T` and `Arc<T>` are `Clone` backends when `T` isn't).
///   `.await` needs a `'static` backend; through a shorter `&T` borrow,
///   finish a view with `.with_metadata()` and a call with `.send()`. When
///   the expansion resolves to near-kit with its `rpc` feature, `C`
///   defaults to `near_kit::Near`;
/// - `impl Contract for Counter`, which `Near::contract::<Counter>(id)` uses
///   to build the client.
///
/// Each method takes `&self` (a view) or `&mut self` with `#[call]`, and at
/// most one argument bound to a plain name (`args: Args`). Generic, `async`
/// and default methods are rejected, as are the names `client`,
/// `contract_id` and `clone`. A method's `#[cfg]` and `#[cfg_attr]`
/// attributes apply to everything generated for it, but `#[call]`,
/// `#[json]` and `#[borsh]` can't go behind `#[cfg_attr]`; for a format that
/// depends on a `cfg`, declare the method twice, each under its own
/// `#[cfg]`. A raw identifier (`fn r#type`) calls the contract method
/// without the `r#` (`type`).
///
/// Everything it names is in near-kit-primitives, so it works without the
/// RPC client (e.g. in contracts built for wasm).
#[proc_macro_attribute]
pub fn contract(attr: TokenStream, item: TokenStream) -> TokenStream {
    let format = parse_macro_input!(attr as SerializationFormat);
    let input = parse_macro_input!(item as ItemTrait);

    match contract_impl(format, input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn contract_impl(format: SerializationFormat, input: ItemTrait) -> syn::Result<TokenStream2> {
    let near_kit = near_kit_path();
    let backend = backend_param();
    let trait_name = &input.ident;
    let client_name = format_ident!("{}Client", trait_name);
    let vis = &input.vis;

    let unsupported = [
        (input.unsafety.map(|t| t.span()), "unsafe traits"),
        (input.modifiers.auto_token.map(|t| t.span()), "auto traits"),
        (
            (!input.generics.params.is_empty()).then(|| input.generics.span()),
            "generic parameters",
        ),
        (
            input.generics.where_clause.as_ref().map(|w| w.span()),
            "where clauses",
        ),
        (
            (!input.supertraits.is_empty()).then(|| input.supertraits.span()),
            "supertraits",
        ),
    ];
    if let Some((span, what)) = unsupported
        .into_iter()
        .find_map(|(span, what)| Some((span?, what)))
    {
        return Err(syn::Error::new(
            span,
            format!("#[near_kit::contract] does not support {what}"),
        ));
    }

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
                generate_view_method(m, format, &near_kit, &backend)
            } else {
                generate_call_method(m, trait_name, &near_kit, &backend)
            }
        })
        .collect();

    let function_call_methods: Vec<TokenStream2> = methods
        .iter()
        .filter(|m| !m.is_view)
        .map(|m| generate_function_call_method(m, format, &near_kit))
        .collect();

    let trait_attrs = &input.attrs;

    let client_doc = format!(
        " Client for the [`{trait_name}`] contract, sending through a backend.\n\n \
         View methods need a `Viewer + Clone` backend; `#[call]` methods need `TxSender + Clone`. \
         For a backend that isn't `Clone`, use `&T` or `Arc<T>`; through a borrow that isn't \
         `'static`, finish a view with `.with_metadata()` and a call with `.send()` instead of \
         `.await`."
    );

    let expanded = quote! {
        #(#trait_attrs)*
        #[derive(Debug, Clone)]
        #vis struct #trait_name;

        impl #trait_name {
            #(#function_call_methods)*
        }

        // `client_struct!` adds the `= Near` default for the backend when near-kit
        // has its client; a `cfg` here would test the caller's features.
        #near_kit::__private::client_struct! {
            #[doc = #client_doc]
            #[derive(Debug, Clone)]
            #vis struct #client_name<#backend> {
                backend: #backend,
                contract_id: #near_kit::AccountId,
            }
        }

        impl<#backend> #client_name<#backend> {
            /// The contract account this client talks to.
            pub fn contract_id(&self) -> &#near_kit::AccountId {
                &self.contract_id
            }

            #(#client_methods)*
        }

        impl #near_kit::Contract for #trait_name {
            type Client<#backend> = #client_name<#backend>;

            fn client<#backend>(backend: #backend, contract_id: #near_kit::AccountId) -> #client_name<#backend> {
                #client_name {
                    backend,
                    contract_id,
                }
            }
        }
    };

    Ok(expanded)
}
