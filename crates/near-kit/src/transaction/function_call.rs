use std::fmt;

use crate::error::Error;
use crate::types::{Action, Gas, IntoGas, IntoNearToken, NearToken};

/// A standalone function call configuration, decoupled from any transaction.
///
/// Use this when you need to pre-build calls and compose them into a transaction
/// later. This is especially useful for dynamic transaction composition (e.g. in
/// a loop) or for batching typed contract calls into a single transaction.
///
/// Note: `FunctionCall` does not capture a receiver/contract account. The call
/// will execute against whichever `receiver_id` is set on the transaction it's
/// added to.
///
/// Available with `default-features = false`; no client or signer is required.
///
/// # Example
///
/// ```rust
/// use near_kit::transaction::FunctionCall;
/// use near_kit::Gas;
///
/// let action = FunctionCall::new("init")
///     .args(serde_json::json!({"owner": "alice.testnet"}))
///     .gas(Gas::from_tgas(50))
///     .into_action()?;
/// # Ok::<(), near_kit::Error>(())
/// ```
pub struct FunctionCall {
    method: String,
    args: Vec<u8>,
    gas: Gas,
    deposit: NearToken,
    construction_error: Option<Error>,
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

    fn defer_error(&mut self, error: Error) {
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
    pub fn args_raw(mut self, args: Vec<u8>) -> Self {
        self.args = args;
        self
    }

    /// Set Borsh-encoded arguments.
    pub fn args_borsh(mut self, args: impl borsh::BorshSerialize) -> Self {
        match borsh::to_vec(&args) {
            Ok(args) => self.args = args,
            Err(error) => self.defer_error(Error::Borsh(error.to_string())),
        }
        self
    }

    /// Set gas limit.
    ///
    /// Defaults to 30 TGas if not set.
    ///
    /// Parsing failures are retained and returned when the call is converted,
    /// sent, built, signed, or delegated.
    pub fn gas(mut self, gas: impl IntoGas) -> Self {
        match gas.into_gas() {
            Ok(gas) => self.gas = gas,
            Err(error) => self.defer_error(error.into()),
        }
        self
    }

    /// Set attached deposit.
    ///
    /// Defaults to zero if not set.
    ///
    /// Parsing failures are retained and returned when the call is converted,
    /// sent, built, signed, or delegated.
    pub fn deposit(mut self, amount: impl IntoNearToken) -> Self {
        match amount.into_near_token() {
            Ok(deposit) => self.deposit = deposit,
            Err(error) => self.defer_error(error.into()),
        }
        self
    }

    /// Convert this call into a transaction action.
    ///
    /// # Errors
    ///
    /// Returns the first serialization, gas, or deposit parsing error captured
    /// while building the call.
    pub fn into_action(self) -> Result<Action, Error> {
        self.try_into()
    }
}

impl TryFrom<FunctionCall> for Action {
    type Error = Error;

    fn try_from(call: FunctionCall) -> Result<Self, Self::Error> {
        if let Some(error) = call.construction_error {
            return Err(error);
        }
        Ok(Action::function_call(
            call.method,
            call.args,
            call.gas,
            call.deposit,
        ))
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

    // FunctionCall tests

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
        assert!(matches!(json_error, Error::Json(_)));

        let borsh_error = FunctionCall::new("method")
            .args_borsh(FailingBorsh)
            .into_action()
            .unwrap_err();
        assert!(matches!(borsh_error, Error::Borsh(_)));
    }

    #[test]
    fn function_call_invalid_gas_and_deposit_are_fallible() {
        let gas_error = FunctionCall::new("method")
            .gas("definitely not gas")
            .into_action()
            .unwrap_err();
        assert!(matches!(gas_error, Error::ParseGas(_)));

        let deposit_error = FunctionCall::new("method")
            .deposit("definitely not NEAR")
            .into_action()
            .unwrap_err();
        assert!(matches!(deposit_error, Error::ParseAmount(_)));
    }

    #[test]
    fn function_call_preserves_the_first_construction_error() {
        let error = FunctionCall::new("method")
            .args(FailingJson)
            .gas("definitely not gas")
            .deposit("definitely not NEAR")
            .into_action()
            .unwrap_err();

        assert!(matches!(error, Error::Json(_)));
    }
}
