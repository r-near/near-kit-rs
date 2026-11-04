//! [`RpcEndpoint`]: one RPC URL plus the headers sent only to it.

use std::fmt;

use super::transport::redact_url;

/// One JSON-RPC endpoint: a URL and the HTTP headers sent only to it, such as
/// a provider's API key.
///
/// [`RpcClient::new`](super::RpcClient::new) and
/// [`Near::custom`](crate::Near::custom) take one, and
/// [`RpcClient::fallback`](super::RpcClient::fallback) and
/// [`NearBuilder::fallback`](crate::NearBuilder::fallback) add more. A `&str`
/// or `String` converts into an endpoint without headers, so a URL can be
/// passed directly.
///
/// Put a provider's key on its endpoint rather than in the default headers
/// of a shared `reqwest::Client`: those go to every endpoint, so a fallback
/// provider would receive the primary's key. An endpoint header replaces a
/// client default header of the same name.
///
/// `Debug` output shows only the URL's scheme and host and the header names,
/// never their values.
///
/// # Example
///
/// ```rust
/// use near_kit::Near;
/// use near_kit::rpc::RpcEndpoint;
///
/// let near = Near::custom(
///     RpcEndpoint::new("https://rpc.mainnet.example.com").bearer("my-api-key"),
///     "mainnet",
/// )
/// .fallback(RpcEndpoint::new("https://rpc.other.example.com").header("x-api-key", "other-key"))
/// .fallback("https://free.rpc.fastnear.com")
/// .build();
///
/// assert_eq!(near.rpc().url(), "https://rpc.mainnet.example.com");
/// assert_eq!(near.rpc().endpoints().len(), 3);
/// ```
#[derive(Clone)]
pub struct RpcEndpoint {
    pub(crate) url: String,
    /// `(name, value)` pairs, at most one per name (compared
    /// case-insensitively).
    pub(crate) headers: Vec<(String, String)>,
}

impl RpcEndpoint {
    /// An endpoint at `url` with no headers of its own.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    /// Send the header `name: value` with every request to this endpoint.
    ///
    /// Setting a name that is already set replaces its value (names compare
    /// case-insensitively). The name and value are checked when a request is
    /// sent: one that isn't a valid HTTP header fails that request with a
    /// non-retryable [`RpcError::Network`](crate::rpc::RpcError::Network).
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let name = name.into();
        let value = value.into();
        match self
            .headers
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(&name))
        {
            Some(entry) => *entry = (name, value),
            None => self.headers.push((name, value)),
        }
        self
    }

    /// Send `Authorization: Bearer <token>` with every request to this
    /// endpoint, replacing any `Authorization` header set before.
    #[must_use]
    pub fn bearer(self, token: impl AsRef<str>) -> Self {
        let value = format!("Bearer {}", token.as_ref());
        self.header("Authorization", value)
    }

    /// The endpoint's URL.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl From<&str> for RpcEndpoint {
    fn from(url: &str) -> Self {
        Self::new(url)
    }
}

impl From<String> for RpcEndpoint {
    fn from(url: String) -> Self {
        Self::new(url)
    }
}

impl From<&String> for RpcEndpoint {
    fn from(url: &String) -> Self {
        Self::new(url.as_str())
    }
}

impl fmt::Debug for RpcEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// The header names, each with a placeholder for its value.
        struct Redacted<'a>(&'a [(String, String)]);

        impl fmt::Debug for Redacted<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_map()
                    .entries(
                        self.0
                            .iter()
                            .map(|(name, _)| (name, format_args!("<redacted>"))),
                    )
                    .finish()
            }
        }

        f.debug_struct("RpcEndpoint")
            // The URL may carry an API key (path or query) or credentials
            // (userinfo), so only the scheme and host are shown.
            .field("url", &redact_url(&self.url))
            .field("headers", &Redacted(&self.headers))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_replaces_a_name_set_before_whatever_its_case() {
        let endpoint = RpcEndpoint::new("https://rpc.example.com")
            .bearer("first")
            .header("X-Api-Key", "a")
            .header("authorization", "Basic b")
            .bearer("second")
            .header("x-api-key", "c");

        assert_eq!(
            endpoint.headers,
            [
                ("Authorization".to_string(), "Bearer second".to_string()),
                ("x-api-key".to_string(), "c".to_string()),
            ]
        );
    }

    #[test]
    fn strings_convert_into_endpoints_without_headers() {
        let owned = String::from("https://b.example.com");
        for endpoint in [
            RpcEndpoint::from("https://a.example.com"),
            RpcEndpoint::from(&owned),
            RpcEndpoint::from(owned.clone()),
        ] {
            assert!(endpoint.url().ends_with(".example.com"));
            assert!(endpoint.headers.is_empty());
        }
    }

    #[test]
    fn debug_shows_header_names_but_not_values_or_url_secrets() {
        let endpoint = RpcEndpoint::new("https://user:PASSWORD@rpc.example.com/v3/PATHKEY?k=QUERY")
            .bearer("TOKEN")
            .header("x-api-key", "HEADERKEY");
        let shown = format!("{endpoint:?}");

        for secret in ["PASSWORD", "user", "PATHKEY", "QUERY", "TOKEN", "HEADERKEY"] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
        assert_eq!(
            shown,
            r#"RpcEndpoint { url: "https://rpc.example.com", headers: {"Authorization": <redacted>, "x-api-key": <redacted>} }"#
        );
    }
}
