//! NEP-141 and NEP-171 token clients.

mod ft;
mod nft;
mod types;

pub use ft::*;
pub use nft::*;
pub use types::*;

use std::str::FromStr;

use serde::de::DeserializeOwned;

use crate::Viewer;
use crate::types::{AccountId, Finality};
use crate::{ContractError, View};

/// Run a JSON view of `method` through a borrowed backend at optimistic
/// finality, sending `args` if there are any.
async fn read<C, T>(
    backend: &C,
    contract_id: &AccountId,
    method: &'static str,
    args: Option<serde_json::Value>,
) -> Result<T, ContractError<C::ViewError>>
where
    C: Viewer,
    T: DeserializeOwned,
{
    let mut view = View::json(backend, contract_id.clone(), method).at(Finality::Optimistic);
    if let Some(args) = args {
        view = view.args(args);
    }
    Ok(view.with_metadata().await?.result)
}

/// A number the contract returns as a JSON string (`U128`/`U64`).
struct Stringified<T>(T);

impl<'de, T> serde::Deserialize<'de> for Stringified<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map(Stringified).map_err(|error| {
            serde::de::Error::custom(format_args!("invalid number {text:?}: {error}"))
        })
    }
}

#[cfg(test)]
pub(crate) mod mock {
    //! A backend that answers views from a table and records what it sends.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use crate::rpc::{BlockReference, ViewFunctionResult};
    use crate::types::{AccountId, Action, CryptoHash};
    use crate::{TxSender, Viewer};

    #[derive(Clone, Default)]
    pub(crate) struct Mock {
        pub views: Arc<Mutex<HashMap<&'static str, serde_json::Value>>>,
        pub seen: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    }

    impl Mock {
        pub fn answer(self, method: &'static str, value: serde_json::Value) -> Self {
            self.views.lock().unwrap().insert(method, value);
            self
        }
    }

    impl Viewer for Mock {
        type ViewError = String;

        async fn view_function(
            &self,
            _contract_id: &AccountId,
            method: &str,
            args: &[u8],
            _block: BlockReference,
        ) -> Result<ViewFunctionResult, String> {
            let args = if args.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_slice(args).unwrap()
            };
            self.seen.lock().unwrap().push((method.to_owned(), args));
            let value = self.views.lock().unwrap().get(method).cloned();
            let value = value.ok_or_else(|| format!("no answer for {method}"))?;
            Ok(ViewFunctionResult::new(
                serde_json::to_vec(&value).unwrap(),
                1,
                CryptoHash::ZERO,
            ))
        }
    }

    impl TxSender for Mock {
        type Output = (AccountId, Vec<Action>);
        type SendError = String;

        fn account_id(&self) -> &AccountId {
            unimplemented!("tokens never ask for the sender")
        }

        async fn send(
            &self,
            receiver_id: AccountId,
            actions: Vec<Action>,
        ) -> Result<Self::Output, String> {
            Ok((receiver_id, actions))
        }
    }

    /// The JSON arguments, deposit and gas of a single sent function call.
    pub(crate) fn sent_call(
        sent: (AccountId, Vec<Action>),
    ) -> (String, serde_json::Value, u128, u64) {
        let (_, actions) = sent;
        assert_eq!(actions.len(), 1);
        match actions.into_iter().next().unwrap() {
            Action::FunctionCall(call) => (
                call.method_name.clone(),
                serde_json::from_slice(&call.args).unwrap(),
                call.deposit.as_yoctonear(),
                call.gas.as_gas(),
            ),
            action => panic!("expected function call, got {action:?}"),
        }
    }
}
