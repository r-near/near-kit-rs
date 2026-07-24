//! Pluggable HTTP transport for the JSON-RPC client.
//!
//! [`RpcClient`](super::rpc::RpcClient) never talks to an HTTP library
//! directly — it drives an [`RpcTransport`] trait object. The built-in
//! implementation is chosen by target:
//!
//! - **Every target except WASI** (native + `wasm32-unknown-unknown`):
//!   [`ReqwestTransport`]. reqwest's fetch backend covers browsers/JS hosts;
//!   its native stack covers everything else.
//! - **`wasm32-wasip2`** with the default-on `wasi-http` feature:
//!   `WasiHttpTransport` (only nameable there), which speaks
//!   `wasi:http/outgoing-handler` through the `wasi` crate's raw bindings.
//!   reqwest has no WASI support (its native stack drags in an `aws-lc-sys` C
//!   cross-compile and hyper's tokio `net`, neither of which builds for wasm),
//!   so a wasip2 component needs a `wasi:http`-native client instead.
//! - **WASI without `wasi-http`**: no built-in transport. The feature split
//!   matters because compiling the built-in transport at all makes the
//!   component import `wasi:http`, which hosts lacking that interface refuse
//!   to instantiate — with only `rpc` enabled, those imports never appear.
//!
//! Custom implementations plug in via
//! [`NearBuilder::transport`](super::NearBuilder::transport) (or
//! [`RpcClient::with_transport_and_retry_config`](super::RpcClient::with_transport_and_retry_config)) —
//! for example a host-call transport on a platform whose runtime proxies RPC
//! traffic instead of exposing raw HTTP.

use std::sync::Arc;

use crate::error::RpcError;
pub use crate::platform::BoxFuture;
use crate::platform::{MaybeSend, MaybeSync};

// The built-in WASI transport speaks the `wasi:http` component-model
// interface, which only exists on WASI Preview 2. On earlier WASI targets,
// fail loudly at build time instead of emitting an artifact whose `wasi:http`
// imports no preview1 host can satisfy.
#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    not(target_env = "p2")
))]
compile_error!(
    "near-kit's `wasi-http` feature requires wasm32-wasip2 (the wasi:http interface does not \
     exist on earlier WASI targets); disable it and supply a custom transport via \
     `NearBuilder::transport` instead"
);

/// A raw HTTP response handed back from an [`RpcTransport`].
///
/// The body is raw bytes, not a `String`: HTTP bodies are bytes on the wire,
/// and keeping the trait encoding-agnostic means transports never have to
/// know that NEAR's JSON-RPC happens to be UTF-8 text. The RPC layer owns the
/// (lossy) UTF-8 decode, exactly as it did when reqwest's `text()` did it.
#[derive(Debug)]
pub struct TransportResponse {
    /// The HTTP status code (e.g. `200`).
    pub status: u16,
    /// The complete response body.
    pub body: Vec<u8>,
}

/// The HTTP layer under [`RpcClient`](super::rpc::RpcClient).
///
/// One method: POST a JSON body, return the status and body — even for non-2xx
/// statuses. The RPC layer interprets the response (including nearcore's
/// convention of well-formed JSON-RPC error bodies on 4xx/5xx), so a transport
/// should only return `Err` when no HTTP response was obtained at all.
///
/// # Errors
///
/// [`RpcError::Network`] is the portable error variant: set `retryable: true`
/// for plausibly-transient failures (DNS, connect, timeout, connection reset)
/// and `false` for deterministic ones (malformed URL, invalid request), and the
/// client's retry/backoff loop will honor it. The built-in
/// [`ReqwestTransport`] instead returns [`RpcError::Http`] so reqwest's own
/// retryability classification keeps applying unchanged.
///
/// # Implementing
///
/// The trait is object-safe, so the future comes back boxed ([`BoxFuture`]);
/// wrap an async block in `Box::pin`. The future may borrow `self` but not
/// `url` or `body` — copy what you need up front. On native targets
/// implementations (and their futures) must be `Send + Sync`; on
/// `wasm32-unknown-unknown` those bounds disappear so browser APIs can be used
/// directly.
///
/// ```rust,ignore
/// struct MyTransport;
///
/// impl RpcTransport for MyTransport {
///     fn post_json(
///         &self,
///         url: &str,
///         body: Vec<u8>,
///     ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
///         let url = url.to_string();
///         Box::pin(async move { /* ... */ })
///     }
/// }
/// ```
pub trait RpcTransport: MaybeSend + MaybeSync {
    /// POST `body` to `url` with `Content-Type: application/json`.
    fn post_json(
        &self,
        url: &str,
        body: Vec<u8>,
    ) -> BoxFuture<'_, Result<TransportResponse, RpcError>>;
}

/// Implement `RpcTransport` for `Arc<T>` where `T: RpcTransport`.
///
/// This covers `Arc<dyn RpcTransport>` (since `dyn RpcTransport:
/// RpcTransport`) as well as concrete types, so an already-shared transport
/// can be passed to [`NearBuilder::transport`](super::NearBuilder::transport)
/// without unwrapping it.
impl<T: RpcTransport + ?Sized> RpcTransport for Arc<T> {
    fn post_json(
        &self,
        url: &str,
        body: Vec<u8>,
    ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
        (**self).post_json(url, body)
    }
}

/// The transport used when the caller doesn't supply one.
///
/// Only compiled when a built-in transport exists for the configuration:
/// reqwest everywhere except WASI, the `wasi:http` transport on wasm32-wasip2
/// with the `wasi-http` feature. A WASI build without `wasi-http` has no
/// default — the caller injects one (`NearBuilder::transport`), and because
/// this function doesn't exist there, nothing can drag `wasi:http` imports
/// into such a component.
#[cfg(any(
    not(all(target_arch = "wasm32", target_os = "wasi")),
    all(feature = "wasi-http", target_env = "p2")
))]
pub(crate) fn default_transport() -> Arc<dyn RpcTransport> {
    #[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
    {
        Arc::new(ReqwestTransport::new())
    }
    #[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
    {
        Arc::new(WasiHttpTransport::new())
    }
}

// ============================================================================
// Off-WASI: reqwest (native + wasm32-unknown-unknown)
// ============================================================================

/// The default [`RpcTransport`] on every target except WASI, backed by
/// [`reqwest::Client`].
///
/// Wrap a preconfigured client (default headers, proxies, TLS, timeouts, ...)
/// with [`ReqwestTransport::with_client`] — or use the
/// [`NearBuilder::http_client`](super::NearBuilder::http_client) shorthand.
#[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
#[derive(Clone, Default)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

#[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
impl ReqwestTransport {
    /// Create a transport with a default `reqwest::Client`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wrap a preconfigured `reqwest::Client`.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

// Manual impl: `reqwest::Client`'s Debug prints its full config, including
// default headers, which may hold API keys.
#[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
impl std::fmt::Debug for ReqwestTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReqwestTransport").finish_non_exhaustive()
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
impl RpcTransport for ReqwestTransport {
    fn post_json(
        &self,
        url: &str,
        body: Vec<u8>,
    ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
        let request = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body);
        Box::pin(async move {
            // Failures pass through as `RpcError::Http`, keeping reqwest's
            // retryability classification (`is_timeout`/`is_connect`) exactly
            // as it was before the transport seam existed.
            let response = request.send().await?;
            let status = response.status().as_u16();
            let body = response.bytes().await?.to_vec();
            Ok(TransportResponse { status, body })
        })
    }
}

// ============================================================================
// WASI (wasm32-wasip2, `wasi-http` feature): wasi:http/outgoing-handler
// ============================================================================

/// The default [`RpcTransport`] on `wasm32-wasip2` (behind the default-on
/// `wasi-http` feature), backed by `wasi:http/outgoing-handler`.
///
/// The host must provide the `wasi:http` interface — e.g. `wasmtime run -S
/// http`, or any runtime targeting the `wasi:http/proxy` world. For WASI
/// hosts *without* `wasi:http`, disable the `wasi-http` feature (keeping
/// `rpc`) and inject the platform's transport via
/// [`NearBuilder::transport`](super::NearBuilder::transport) instead.
///
/// The exchange is *blocking*: the guest parks on a wasi pollable until the
/// response arrives, so exactly one request is in flight at a time. That is
/// the natural shape for a single-threaded component; concurrent RPC calls
/// from the same guest serialize rather than interleave.
#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2"
))]
#[derive(Clone, Debug, Default)]
pub struct WasiHttpTransport;

#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2"
))]
impl WasiHttpTransport {
    /// Create a new wasi:http transport.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2"
))]
impl RpcTransport for WasiHttpTransport {
    fn post_json(
        &self,
        url: &str,
        body: Vec<u8>,
    ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
        Box::pin(wasi_http::Exchange::new(url, body))
    }
}

#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2"
))]
mod wasi_http {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use wasi::http::outgoing_handler;
    use wasi::http::types::{
        ErrorCode, Fields, IncomingBody, Method, OutgoingBody, OutgoingRequest, Scheme,
    };
    use wasi::io::streams::StreamError;

    use super::TransportResponse;
    use crate::error::RpcError;

    /// `wasi:io` streams cap a single `blocking-write-and-flush` at 4096 bytes.
    const WRITE_CHUNK: usize = 4096;
    /// Bytes to request per `blocking-read` while draining the response body.
    const READ_CHUNK: u64 = 16 * 1024;

    /// A future that runs the whole blocking `wasi:http` exchange inside a
    /// single `poll` and returns `Ready`.
    ///
    /// Send-safety: `BoxFuture` requires `Send` on this target (only
    /// `wasm32-unknown-unknown` relaxes it), but every `wasi` resource handle
    /// (`OutgoingRequest`, streams, pollables) is `!Send`. Doing the exchange —
    /// dispatch, pollable block, body drain — synchronously inside one `poll`
    /// means those handles live and die in a single stack frame and are never
    /// stored in the future. `Exchange` itself holds only a `String` and a
    /// `Vec<u8>`, so it is `Send` by construction, with no `unsafe`.
    pub(super) struct Exchange {
        request: Option<(String, Vec<u8>)>,
    }

    impl Exchange {
        pub(super) fn new(url: &str, body: Vec<u8>) -> Self {
            Self {
                request: Some((url.to_string(), body)),
            }
        }
    }

    impl Future for Exchange {
        type Output = Result<TransportResponse, RpcError>;

        fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            let (url, body) = self
                .request
                .take()
                .expect("Exchange future polled after completion");
            Poll::Ready(exchange(&url, &body))
        }
    }

    /// The synchronous `wasi:http/outgoing-handler` exchange.
    fn exchange(url: &str, body: &[u8]) -> Result<TransportResponse, RpcError> {
        let UrlParts {
            scheme,
            authority,
            path_with_query,
            authorization,
        } = split_url(url)?;

        let mut header_list = vec![
            ("content-type".to_string(), b"application/json".to_vec()),
            (
                "content-length".to_string(),
                body.len().to_string().into_bytes(),
            ),
        ];
        if let Some(value) = authorization {
            header_list.push(("authorization".to_string(), value));
        }
        // Rejected headers mean a malformed request — not retryable.
        let headers = Fields::from_list(&header_list).map_err(|e| {
            RpcError::network(
                format!("wasi:http rejected request headers: {e:?}"),
                None,
                false,
            )
        })?;

        let request = OutgoingRequest::new(headers);
        request
            .set_method(&Method::Post)
            .map_err(|()| RpcError::network("wasi:http rejected method POST", None, false))?;
        request
            .set_scheme(Some(&scheme))
            .map_err(|()| invalid_url(url, "unsupported scheme"))?;
        request
            .set_authority(Some(&authority))
            .map_err(|()| invalid_url(url, "invalid authority"))?;
        request
            .set_path_with_query(Some(&path_with_query))
            .map_err(|()| invalid_url(url, "invalid path"))?;

        // Take the body handle *before* `handle` consumes the request resource.
        let out_body = request
            .body()
            .map_err(|()| RpcError::network("wasi:http request body already taken", None, false))?;

        let future_response = outgoing_handler::handle(request, None).map_err(|code| {
            RpcError::network(
                format!("wasi:http dispatch failed: {code:?}"),
                None,
                is_retryable_error_code(&code),
            )
        })?;

        // Write the request body in the 4 KiB chunks `wasi:io` allows. The
        // child stream must be dropped before finishing the body (dropping a
        // still-open child traps in some hosts, so the scope is explicit).
        {
            let stream = out_body.write().map_err(|()| {
                RpcError::network("wasi:http request body stream unavailable", None, false)
            })?;
            for chunk in body.chunks(WRITE_CHUNK) {
                stream.blocking_write_and_flush(chunk).map_err(|e| {
                    RpcError::network(
                        format!("wasi:http request body write failed: {}", stream_err(e)),
                        None,
                        true,
                    )
                })?;
            }
        }
        OutgoingBody::finish(out_body, None).map_err(|code| {
            RpcError::network(
                format!("wasi:http finishing request body failed: {code:?}"),
                None,
                is_retryable_error_code(&code),
            )
        })?;

        // Block the (single-threaded) guest until the response is available.
        let pollable = future_response.subscribe();
        pollable.block();
        drop(pollable);

        let response = match future_response.get() {
            Some(Ok(Ok(response))) => response,
            // The host-reported failure (DNS, connect, TLS, ...) — the
            // wasi:http analogue of a reqwest connect error.
            Some(Ok(Err(code))) => {
                return Err(RpcError::network(
                    format!("wasi:http request failed: {code:?}"),
                    None,
                    is_retryable_error_code(&code),
                ));
            }
            Some(Err(())) => {
                return Err(RpcError::network(
                    "wasi:http response consumed twice",
                    None,
                    false,
                ));
            }
            None => {
                return Err(RpcError::network(
                    "wasi:http response not ready after blocking",
                    None,
                    true,
                ));
            }
        };

        // Non-2xx is not an error here: return the status and body and let
        // the RPC layer interpret them (nearcore sends JSON-RPC error bodies
        // with 4xx/5xx statuses).
        let status = response.status();

        let incoming_body = response.consume().map_err(|()| {
            RpcError::network("wasi:http response body already consumed", None, false)
        })?;
        let mut buf = Vec::new();
        {
            let stream = incoming_body.stream().map_err(|()| {
                RpcError::network("wasi:http response body stream unavailable", None, false)
            })?;
            loop {
                match stream.blocking_read(READ_CHUNK) {
                    Ok(chunk) => buf.extend_from_slice(&chunk),
                    Err(StreamError::Closed) => break,
                    Err(e @ StreamError::LastOperationFailed(_)) => {
                        return Err(RpcError::network(
                            format!("wasi:http response body read failed: {}", stream_err(e)),
                            None,
                            true,
                        ));
                    }
                }
            }
        }
        // Finish the incoming body (drops the trailers future we never read).
        let _ = IncomingBody::finish(incoming_body);

        Ok(TransportResponse { status, body: buf })
    }

    /// The `wasi:http` request parts extracted from an RPC endpoint URL.
    struct UrlParts {
        scheme: Scheme,
        /// Host (and optional port), with any URL userinfo stripped.
        authority: String,
        path_with_query: String,
        /// `Authorization` header value synthesized from URL userinfo
        /// (`https://user:pass@host/`), as reqwest does on other targets.
        authorization: Option<Vec<u8>>,
    }

    /// Split a URL into the `wasi:http` request parts. (No `url`-crate
    /// dependency; RPC endpoint URLs are simple enough for direct splitting.)
    fn split_url(url: &str) -> Result<UrlParts, RpcError> {
        let (scheme_raw, rest) = url
            .split_once("://")
            .ok_or_else(|| invalid_url(url, "missing scheme"))?;
        // Scheme names are case-insensitive (the `url` crate lowercases them
        // on other targets). Anything but http/https is deterministically
        // wrong for an RPC endpoint — reject it here, non-retryably, instead
        // of letting the host fail the dispatch with a retryable-looking
        // protocol error.
        let scheme = if scheme_raw.eq_ignore_ascii_case("http") {
            Scheme::Http
        } else if scheme_raw.eq_ignore_ascii_case("https") {
            Scheme::Https
        } else {
            return Err(invalid_url(url, "scheme must be http or https"));
        };
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(authority_end);
        // URL userinfo is not part of the wire-format authority: like reqwest,
        // turn `user:pass@host` into an `Authorization: Basic` header.
        let (authorization, authority) = match authority.rsplit_once('@') {
            Some((userinfo, host)) => (basic_auth(userinfo), host),
            None => (None, authority),
        };
        if authority.is_empty() {
            return Err(invalid_url(url, "missing host"));
        }
        // Fragments are client-side only; never send them.
        let tail = tail.split('#').next().unwrap_or_default();
        let path_with_query = if tail.is_empty() {
            "/".to_string()
        } else if tail.starts_with('?') {
            format!("/{tail}")
        } else {
            tail.to_string()
        };
        Ok(UrlParts {
            scheme,
            authority: authority.to_string(),
            path_with_query,
            authorization,
        })
    }

    /// `Authorization: Basic` value for a URL userinfo segment (`user` or
    /// `user:pass`). Both halves are percent-decoded before base64-encoding,
    /// mirroring reqwest's conversion on other targets.
    fn basic_auth(userinfo: &str) -> Option<Vec<u8>> {
        if userinfo.is_empty() {
            return None;
        }
        let (user, pass) = match userinfo.split_once(':') {
            Some((user, pass)) => (user, Some(pass)),
            None => (userinfo, None),
        };
        let mut credentials = percent_decode(user);
        credentials.push(b':');
        if let Some(pass) = pass {
            credentials.extend(percent_decode(pass));
        }
        Some(format!("Basic {}", STANDARD.encode(credentials)).into_bytes())
    }

    /// Minimal percent-decoding for URL userinfo (`%40` → `@`, ...). Invalid
    /// escapes pass through verbatim, matching lenient URL-parser behavior.
    fn percent_decode(s: &str) -> Vec<u8> {
        fn hex_val(b: u8) -> Option<u8> {
            match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                b'A'..=b'F' => Some(b - b'A' + 10),
                _ => None,
            }
        }
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%'
                && let (Some(hi), Some(lo)) = (
                    bytes.get(i + 1).copied().and_then(hex_val),
                    bytes.get(i + 2).copied().and_then(hex_val),
                )
            {
                out.push((hi << 4) | lo);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        out
    }

    /// A malformed URL is deterministic — never retryable.
    fn invalid_url(url: &str, reason: &str) -> RpcError {
        RpcError::network(format!("invalid RPC URL `{url}`: {reason}"), None, false)
    }

    fn stream_err(e: StreamError) -> String {
        match e {
            StreamError::Closed => "stream closed".to_string(),
            StreamError::LastOperationFailed(err) => err.to_debug_string(),
        }
    }

    /// Retryability of a `wasi:http` [`ErrorCode`], mirroring the reqwest
    /// classification (`is_timeout`/`is_connect`): failures to *reach or keep
    /// talking to* the server (DNS, connect, TLS, timeouts, protocol hiccups)
    /// are transient and worth retrying; codes that say the *request itself*
    /// is unacceptable are deterministic and are not.
    fn is_retryable_error_code(code: &ErrorCode) -> bool {
        !matches!(
            code,
            ErrorCode::HttpRequestDenied
                | ErrorCode::HttpRequestLengthRequired
                | ErrorCode::HttpRequestBodySize(_)
                | ErrorCode::HttpRequestMethodInvalid
                | ErrorCode::HttpRequestUriInvalid
                | ErrorCode::HttpRequestUriTooLong
                | ErrorCode::HttpRequestHeaderSectionSize(_)
                | ErrorCode::HttpRequestHeaderSize(_)
                | ErrorCode::HttpRequestTrailerSectionSize(_)
                | ErrorCode::HttpRequestTrailerSize(_)
                | ErrorCode::LoopDetected
                | ErrorCode::ConfigurationError
        )
    }
}
