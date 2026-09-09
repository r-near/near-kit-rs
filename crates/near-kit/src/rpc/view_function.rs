use std::fmt;

use crate::error::Error;
use crate::types::ViewFunctionResult;

/// A typed view description independent of a client or contract account.
///
/// Construct and decode views with `default-features = false`. The `rpc` feature
/// additionally provides `fetch` for execution through `Near`.
/// Argument serialization is fallible and happens before execution.
///
/// # Example
///
/// ```rust
/// use near_kit::rpc::ViewFunction;
///
/// let view = ViewFunction::<u64>::json("get_balance")
///     .args(serde_json::json!({"account_id": "alice.near"}))?;
/// assert_eq!(view.method_name(), "get_balance");
/// // Another backend can execute view.method_name() with view.args_bytes(),
/// // then pass its raw ViewFunctionResult to view.decode(response).
/// # Ok::<(), near_kit::Error>(())
/// ```
pub struct ViewFunction<T> {
    method: String,
    args: Vec<u8>,
    decode: fn(&ViewFunctionResult) -> Result<T, Error>,
}

impl<T> fmt::Debug for ViewFunction<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewFunction")
            .field("method", &self.method)
            .field("args_len", &self.args.len())
            .finish_non_exhaustive()
    }
}

impl<T: serde::de::DeserializeOwned> ViewFunction<T> {
    /// Describe a JSON-returning view. Arguments default to the empty JSON object.
    pub fn json(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            args: b"{}".to_vec(),
            decode: |response| Ok(response.json()?),
        }
    }
}

impl<T: borsh::BorshDeserialize> ViewFunction<T> {
    /// Describe a Borsh-returning view. Arguments default to empty bytes.
    pub fn borsh(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            args: Vec::new(),
            decode: |response| response.borsh().map_err(|e| Error::Borsh(e.to_string())),
        }
    }
}

impl<T> ViewFunction<T> {
    /// Encode JSON arguments, independently of the result encoding.
    pub fn args(mut self, args: impl serde::Serialize) -> Result<Self, Error> {
        self.args = serde_json::to_vec(&args)?;
        Ok(self)
    }

    /// Encode Borsh arguments, independently of the result encoding.
    pub fn args_borsh(mut self, args: impl borsh::BorshSerialize) -> Result<Self, Error> {
        self.args = borsh::to_vec(&args).map_err(|e| Error::Borsh(e.to_string()))?;
        Ok(self)
    }

    /// Supply already encoded arguments.
    pub fn args_raw(mut self, args: Vec<u8>) -> Self {
        self.args = args;
        self
    }

    /// Contract method to execute.
    pub fn method_name(&self) -> &str {
        &self.method
    }

    /// Complete encoded arguments to pass to the backend.
    pub fn args_bytes(&self) -> &[u8] {
        &self.args
    }

    /// Decode a raw backend response, retaining its block context and logs.
    pub fn decode(&self, response: ViewFunctionResult) -> Result<ViewFunctionResult<T>, Error> {
        Ok(ViewFunctionResult {
            result: (self.decode)(&response)?,
            logs: response.logs,
            block_height: response.block_height,
            block_hash: response.block_hash,
        })
    }

    /// Execute through near-kit at the selected block and decode the response.
    ///
    /// The account and block are execution choices, not part of the method definition.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// use near_kit::{Near, rpc::{BlockReference, ViewFunction}};
    /// let near = Near::testnet().build();
    /// let result = ViewFunction::<u64>::json("get_count")
    ///     .fetch(&near, "counter.testnet", BlockReference::final_()).await?;
    /// println!("{} at block {}", result.result, result.block_height);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "rpc")]
    pub async fn fetch(
        &self,
        near: &crate::Near,
        account: impl crate::types::TryIntoAccountId,
        block: crate::types::BlockReference,
    ) -> Result<ViewFunctionResult<T>, Error> {
        let account = account.try_into_account_id()?;
        self.decode(
            near.rpc()
                .view_function(&account, &self.method, &self.args, block)
                .await?,
        )
    }
}
