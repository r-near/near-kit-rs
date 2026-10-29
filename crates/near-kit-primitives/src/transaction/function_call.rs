use std::fmt;

use crate::error::CodecError;
use crate::types::{Action, FunctionCallAction, Gas, NearToken};

/// A standalone function call configuration, decoupled from any transaction.
///
/// Pre-build calls and compose them into a transaction later, for example in a
/// loop or to batch typed contract calls. It has no receiver: the call runs
/// against the `receiver_id` of the transaction it's added to. No client or
/// signer is required.
///
/// # Example
///
/// ```rust
/// use near_kit_primitives::transaction::FunctionCall;
/// use near_kit_primitives::Gas;
///
/// let action = FunctionCall::new("init")
///     .args(serde_json::json!({"owner": "alice.testnet"}))
///     .gas(Gas::from_tgas(50))
///     .into_action()?;
/// # Ok::<(), near_kit_primitives::transaction::CodecError>(())
/// ```
pub struct FunctionCall {
    method: String,
    args: Vec<u8>,
    gas: Gas,
    deposit: NearToken,
    construction_error: Option<CodecError>,
}

impl fmt::Debug for FunctionCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FunctionCall")
            .field("method", &self.method)
            .field("args_len", &self.args.len())
            .field("gas", &self.gas)
            .field("deposit", &self.deposit)
            .field("construction_error", &self.construction_error)
            .finish()
    }
}

impl FunctionCall {
    /// Create a new function call for the given method name.
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            args: Vec::new(),
            gas: Gas::from_tgas(30),
            deposit: NearToken::ZERO,
            construction_error: None,
        }
    }

    fn defer_error(&mut self, error: CodecError) {
        if self.construction_error.is_none() {
            self.construction_error = Some(error);
        }
    }

    /// Set JSON arguments.
    pub fn args(mut self, args: impl serde::Serialize) -> Self {
        match serde_json::to_vec(&args) {
            Ok(args) => self.args = args,
            Err(error) => self.defer_error(error.into()),
        }
        self
    }

    /// Set raw byte arguments.
    pub fn args_raw(mut self, args: impl Into<Vec<u8>>) -> Self {
        self.args = args.into();
        self
    }

    /// Set Borsh-encoded arguments.
    pub fn args_borsh(mut self, args: impl borsh::BorshSerialize) -> Self {
        match borsh::to_vec(&args) {
            Ok(args) => self.args = args,
            Err(error) => self.defer_error(CodecError::Borsh(error)),
        }
        self
    }

    /// Set the gas limit.
    ///
    /// Defaults to 30 Tgas if not set.
    pub fn gas(mut self, gas: Gas) -> Self {
        self.gas = gas;
        self
    }

    /// Set the attached deposit.
    ///
    /// Defaults to zero if not set.
    pub fn deposit(mut self, deposit: NearToken) -> Self {
        self.deposit = deposit;
        self
    }

    /// Convert this call into a transaction action.
    ///
    /// # Errors
    ///
    /// Returns the first argument serialization error captured while
    /// building the call.
    pub fn into_action(self) -> Result<Action, CodecError> {
        self.try_into()
    }
}

impl TryFrom<FunctionCall> for FunctionCallAction {
    type Error = CodecError;

    fn try_from(call: FunctionCall) -> Result<Self, Self::Error> {
        if let Some(error) = call.construction_error {
            return Err(error);
        }
        Ok(FunctionCallAction {
            method_name: call.method,
            args: call.args,
            gas: call.gas,
            deposit: call.deposit,
        })
    }
}

impl TryFrom<FunctionCall> for Action {
    type Error = CodecError;

    fn try_from(call: FunctionCall) -> Result<Self, Self::Error> {
        FunctionCallAction::try_from(call).map(Action::FunctionCall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Write};

    struct FailingJson;

    impl serde::Serialize for FailingJson {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("deliberate JSON failure"))
        }
    }

    struct FailingBorsh;

    impl borsh::BorshSerialize for FailingBorsh {
        fn serialize<W: Write>(&self, _writer: &mut W) -> io::Result<()> {
            Err(io::Error::other("deliberate Borsh failure"))
        }
    }

    #[test]
    fn function_call_into_action() {
        let call = FunctionCall::new("init")
            .args(serde_json::json!({"owner": "alice.testnet"}))
            .gas(Gas::from_tgas(50))
            .deposit(NearToken::from_near(1));

        let action = call.into_action().unwrap();
        match &action {
            Action::FunctionCall(fc) => {
                assert_eq!(fc.method_name, "init");
                assert_eq!(
                    fc.args,
                    serde_json::to_vec(&serde_json::json!({"owner": "alice.testnet"})).unwrap()
                );
                assert_eq!(fc.gas, Gas::from_tgas(50));
                assert_eq!(fc.deposit, NearToken::from_near(1));
            }
            other => panic!("expected FunctionCall, got {:?}", other),
        }
    }

    #[test]
    fn function_call_defaults() {
        let call = FunctionCall::new("method");
        let action = call.into_action().unwrap();
        match &action {
            Action::FunctionCall(fc) => {
                assert_eq!(fc.method_name, "method");
                assert!(fc.args.is_empty());
                assert_eq!(fc.gas, Gas::from_tgas(30));
                assert_eq!(fc.deposit, NearToken::ZERO);
            }
            other => panic!("expected FunctionCall, got {:?}", other),
        }
    }

    #[test]
    fn function_call_serialization_errors_are_fallible() {
        let json_error = FunctionCall::new("method")
            .args(FailingJson)
            .into_action()
            .unwrap_err();
        assert!(matches!(json_error, CodecError::Json(_)));

        let borsh_error = FunctionCall::new("method")
            .args_borsh(FailingBorsh)
            .into_action()
            .unwrap_err();
        assert!(matches!(borsh_error, CodecError::Borsh(_)));

        // The first error wins.
        let first = FunctionCall::new("method")
            .args(FailingJson)
            .args_borsh(FailingBorsh)
            .into_action()
            .unwrap_err();
        assert!(matches!(first, CodecError::Json(_)));
    }
}
