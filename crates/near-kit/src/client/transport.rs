//! The [`RpcTransport`] trait and the built-in transport for each target.

use std::sync::Arc;

use crate::error::RpcError;
use crate::{MaybeSend, MaybeSync};
pub use near_kit_primitives::__private::BoxFuture;

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

/// `url` as it may be shown in logs, errors and `Debug` output: only
/// `scheme://host[:port]`. RPC providers put API keys in the query string
/// (`?apikey=...`), in the path (`/v3/<key>`, `/<token>/`) and in `user:pass@`
/// userinfo, so all of those are left out.
///
/// The userinfo is everything up to the last `@` before the path, even when
/// it holds an unencoded `?` or `#`: `https://user:p?ss@host/` shows as
/// `https://host`, not as `https://user:p`.
pub(crate) fn redact_url(url: &str) -> String {
    let authority_start = url.find("://").map_or(0, |i| i + 3);
    let rest = &url[authority_start..];
    let before_path = &rest[..rest.find('/').unwrap_or(rest.len())];
    let host = before_path
        .rfind('@')
        .map_or(before_path, |at| &before_path[at + 1..]);
    let host = &host[..host.find(['?', '#']).unwrap_or(host.len())];
    format!("{}{host}", &url[..authority_start])
}

/// A raw HTTP response handed back from an [`RpcTransport`].
///
/// The body is raw bytes, not a `String`: HTTP bodies are bytes on the wire,
/// and keeping the trait encoding-agnostic means transports never have to
/// know that NEAR's JSON-RPC happens to be UTF-8 text. The RPC layer owns the
/// (lossy) UTF-8 decode, exactly as it did when reqwest's `text()` did it.
///
/// Build one with [`new`](Self::new): the struct is `#[non_exhaustive]`, so
/// fields (such as response headers) can be added later without breaking
/// transports.
#[derive(Debug)]
#[non_exhaustive]
pub struct TransportResponse {
    /// The HTTP status code (e.g. `200`).
    pub status: u16,
    /// The complete response body.
    pub body: Vec<u8>,
}

impl TransportResponse {
    /// A response with HTTP status `status` and the complete `body`.
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self { status, body }
    }
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
/// ```rust
/// use near_kit::Near;
/// use near_kit::rpc::{BoxFuture, RpcError, RpcTransport, TransportResponse};
///
/// /// Answers every request with the same JSON-RPC body.
/// struct Canned(&'static str);
///
/// impl RpcTransport for Canned {
///     fn post_json(
///         &self,
///         _url: &str,
///         _body: Vec<u8>,
///     ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
///         // Copy what the future needs; it may borrow `self` only.
///         let body = self.0.as_bytes().to_vec();
///         Box::pin(async move { Ok(TransportResponse::new(200, body)) })
///     }
/// }
///
/// let near = Near::custom("http://localhost:3030", "localnet")
///     .transport(Canned(r#"{"jsonrpc":"2.0","id":0,"result":null}"#))
///     .build();
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

#[cfg(any(
    test,
    all(
        target_arch = "wasm32",
        target_os = "wasi",
        not(all(feature = "wasi-http", target_env = "p2"))
    )
))]
mod missing_wasi_http {
    use super::{BoxFuture, RpcError, RpcTransport, TransportResponse};

    /// The erroring transport for WASI builds without `wasi-http`.
    #[derive(Clone, Debug, Default)]
    pub(super) struct MissingWasiHttpTransport;

    impl RpcTransport for MissingWasiHttpTransport {
        fn post_json(
            &self,
            _url: &str,
            _body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            Box::pin(async {
                Err(RpcError::network(
                    "no HTTP transport is available on WASI without the `wasi-http` feature; \
                     supply one with `NearBuilder::transport`",
                    None,
                    false,
                ))
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn missing_wasi_http_transport_returns_typed_non_retryable_error() {
            let error = futures::executor::block_on(
                MissingWasiHttpTransport.post_json("https://rpc.testnet.near.org", Vec::new()),
            )
            .unwrap_err();

            assert!(matches!(
                error,
                RpcError::Network {
                    status_code: None,
                    retryable: false,
                    ..
                }
            ));
        }
    }
}

/// The transport used when the caller doesn't supply one.
///
/// This is reqwest everywhere except WASI, and the `wasi:http` transport on
/// wasm32-wasip2 with the `wasi-http` feature. WASI without `wasi-http` gets a
/// non-retryable error transport, so client construction remains infallible
/// without importing `wasi:http`; callers inject a transport to perform RPC.
pub(crate) fn default_transport() -> Arc<dyn RpcTransport> {
    #[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
    {
        Arc::new(ReqwestTransport::new())
    }
    #[cfg(all(
        feature = "wasi-http",
        target_arch = "wasm32",
        target_os = "wasi",
        target_env = "p2"
    ))]
    {
        Arc::new(WasiHttpTransport::new())
    }
    #[cfg(all(
        target_arch = "wasm32",
        target_os = "wasi",
        not(all(feature = "wasi-http", target_env = "p2"))
    ))]
    {
        Arc::new(missing_wasi_http::MissingWasiHttpTransport)
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
pub use reqwest_backend::ReqwestTransport;

#[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
mod reqwest_backend {
    use super::{BoxFuture, RpcError, RpcTransport, TransportResponse};

    /// The default [`RpcTransport`] on every target except WASI, backed by
    /// [`reqwest::Client`].
    ///
    /// Wrap a preconfigured client (default headers, proxies, TLS, timeouts, ...)
    /// with [`ReqwestTransport::with_client`] and pass it to
    /// [`NearBuilder::transport`](crate::NearBuilder::transport). Build it with
    /// [`near_kit::reqwest`](crate::reqwest), the reqwest near-kit is built
    /// with, so the versions match.
    #[derive(Clone, Default)]
    pub struct ReqwestTransport {
        client: reqwest::Client,
    }

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
    impl std::fmt::Debug for ReqwestTransport {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ReqwestTransport").finish_non_exhaustive()
        }
    }

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
                // as it was before the transport seam existed. The `From` impl
                // drops the URL, which may carry an API key.
                let response = request.send().await?;
                let status = response.status().as_u16();
                let body = response.bytes().await?.to_vec();
                Ok(TransportResponse::new(status, body))
            })
        }
    }
}

// reqwest doesn't build for WASI (its native stack needs aws-lc-sys and
// tokio's `net`), so wasip2 components speak `wasi:http` directly.
#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2"
))]
pub use wasi_http::WasiHttpTransport;

#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2"
))]
mod wasi_http {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use wasi::http::outgoing_handler;
    use wasi::http::types::{
        ErrorCode, Fields, IncomingBody, Method, OutgoingBody, OutgoingRequest, Scheme,
    };
    use wasi::io::streams::StreamError;

    use super::{BoxFuture, RpcError, RpcTransport, TransportResponse};

    /// `wasi:io` streams cap a single `blocking-write-and-flush` at 4096 bytes.
    const WRITE_CHUNK: usize = 4096;
    /// Bytes to request per `blocking-read` while draining the response body.
    const READ_CHUNK: u64 = 16 * 1024;

    /// The built-in [`RpcTransport`] on `wasm32-wasip2` (behind the opt-in
    /// `wasi-http` feature), backed by `wasi:http/outgoing-handler`.
    ///
    /// The host must provide the `wasi:http` interface — e.g. `wasmtime run -S
    /// http`, or any runtime targeting the `wasi:http/proxy` world. For WASI
    /// hosts *without* `wasi:http`, disable the `wasi-http` feature (keeping
    /// `rpc`) and inject the platform's transport via
    /// [`NearBuilder::transport`](crate::NearBuilder::transport) instead.
    ///
    /// The exchange is *blocking*: the guest parks on a wasi pollable until the
    /// response arrives, so exactly one request is in flight at a time. That is
    /// the natural shape for a single-threaded component; concurrent RPC calls
    /// from the same guest serialize rather than interleave.
    #[derive(Clone, Debug, Default)]
    pub struct WasiHttpTransport;

    impl WasiHttpTransport {
        /// Create a new wasi:http transport.
        pub fn new() -> Self {
            Self
        }
    }

    impl RpcTransport for WasiHttpTransport {
        fn post_json(
            &self,
            url: &str,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            // `exchange` never awaits, so the `!Send` wasi handles it creates
            // never live across a yield and the future stays `Send`.
            let url = url.to_owned();
            Box::pin(async move { exchange(&url, &body) })
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
                if let Err(e) = stream.blocking_write_and_flush(chunk) {
                    // The host closes the body stream when the request itself
                    // failed (connection refused, DNS, TLS, ...). That cause
                    // is in the response future, so report it when it's there.
                    let pollable = future_response.subscribe();
                    pollable.block();
                    drop(pollable);
                    return Err(match future_response.get() {
                        Some(Ok(Err(code))) => request_failed(&code),
                        _ => RpcError::network(
                            format!("wasi:http request body write failed: {}", stream_err(e)),
                            None,
                            true,
                        ),
                    });
                }
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
            Some(Ok(Err(code))) => return Err(request_failed(&code)),
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

        Ok(TransportResponse::new(status, buf))
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

    /// A malformed URL is deterministic — never retryable. The message shows
    /// only the scheme and host, since the rest may carry an API key.
    fn invalid_url(url: &str, reason: &str) -> RpcError {
        let shown = super::redact_url(url);
        RpcError::network(format!("invalid RPC URL `{shown}`: {reason}"), None, false)
    }

    /// The host-reported failure of a request (DNS, connect, TLS, ...).
    fn request_failed(code: &ErrorCode) -> RpcError {
        RpcError::network(
            format!("wasi:http request failed: {code:?}"),
            None,
            is_retryable_error_code(code),
        )
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
