//! What `#[contract]` generates, checked at compile time: each module
//! declares a contract and its test names the generated items with their
//! expected types.

/// A JSON contract (the default format), its client and constructors.
mod json_contract {
    use near_kit::signer::{InMemorySigner, SecretKey};
    use near_kit::transaction::FunctionCall;
    use near_kit::*;
    use near_kit::{Call, View};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Message {
        pub text: String,
    }

    #[near_kit::contract]
    pub trait JsonGuestbook {
        /// Every message, oldest first.
        fn get_messages(&self) -> Vec<Message>;
        fn total_messages(&self) -> u32;

        /// Append a message.
        #[call]
        fn add_message(&mut self, text: String);
    }

    #[test]
    fn compiles() {
        let near = Near::testnet().build();
        // `JsonGuestbookClient` defaults its backend to `Near`.
        let client: JsonGuestbookClient =
            near.contract::<JsonGuestbook>(account!("guestbook.testnet"));
        let _: &AccountId = client.contract_id();

        let _view: View<Near, Vec<Message>> = client.get_messages();
        let _view: View<Near, u32> = client
            .total_messages()
            .at(near_kit::rpc::Finality::Optimistic);
        // Calls need a sender: the same client over `SigningNear`.
        let account: SigningNear = near.with_signer(InMemorySigner::new(
            account!("alice.testnet"),
            SecretKey::generate_ed25519(),
        ));
        let signing_client: JsonGuestbookClient<SigningNear> =
            account.contract::<JsonGuestbook>(account!("guestbook.testnet"));
        let _call: Call<SigningNear> = signing_client
            .add_message("hello".to_string())
            .gas(Gas::from_tgas(50))
            .deposit(NearToken::from_yoctonear(1));

        // The client can also be built from the trait, for any backend.
        let _: JsonGuestbookClient<Near> =
            <JsonGuestbook as Contract>::client(near.clone(), account!("guestbook.testnet"));

        let _fc: FunctionCall = JsonGuestbook::add_message("composable".to_string());
        let _tx = account
            .transaction(account!("guestbook.testnet"))
            .add_action(JsonGuestbook::add_message("composed".to_string()).gas(Gas::from_tgas(50)));
    }
}

/// Per-method `#[json]`/`#[borsh]` overrides on either contract default.
mod mixed_format_contract {
    use near_kit::signer::{InMemorySigner, SecretKey};
    use near_kit::transaction::FunctionCall;
    use near_kit::*;
    use near_kit::{Call, View};

    #[near_kit::contract]
    pub trait MixedContract {
        fn get_json_value(&self) -> u64;

        #[borsh]
        fn get_borsh_value(&self) -> u64;

        #[call]
        fn set_json_value(&mut self);

        #[call]
        #[borsh]
        fn set_borsh_value(&mut self);
    }

    #[near_kit::contract(borsh)]
    pub trait MixedContractBorshDefault {
        fn get_borsh_value(&self) -> u64;

        #[json]
        fn get_json_value(&self) -> u64;

        #[call]
        fn set_borsh_value(&mut self);

        #[call]
        #[json]
        fn set_json_value(&mut self);
    }

    #[test]
    fn compiles() {
        let near = Near::testnet().build();
        let account: SigningNear = near.with_signer(InMemorySigner::new(
            account!("alice.testnet"),
            SecretKey::generate_ed25519(),
        ));

        let client = near.contract::<MixedContract>(account!("contract.testnet"));
        let signing_client = account.contract::<MixedContract>(account!("contract.testnet"));
        let _: View<Near, u64> = client.get_json_value();
        let _: View<Near, u64> = client.get_borsh_value();
        let _: Call<SigningNear> = signing_client.set_json_value();
        let _: Call<SigningNear> = signing_client.set_borsh_value();

        let client = near.contract::<MixedContractBorshDefault>(account!("contract.testnet"));
        let signing_client =
            account.contract::<MixedContractBorshDefault>(account!("contract.testnet"));
        let _: View<Near, u64> = client.get_borsh_value();
        let _: View<Near, u64> = client.get_json_value();
        let _: Call<SigningNear> = signing_client.set_borsh_value();
        let _: Call<SigningNear> = signing_client.set_json_value();

        let _: FunctionCall = MixedContract::set_json_value();
        let _: FunctionCall = MixedContract::set_borsh_value();
        let _: FunctionCall = MixedContractBorshDefault::set_borsh_value();
        let _: FunctionCall = MixedContractBorshDefault::set_json_value();
    }
}

/// A method's `#[cfg]` and `#[cfg_attr]` apply to everything generated for
/// it, so a configured-out method may name items that don't exist.
mod cfg_method {
    use near_kit::transaction::FunctionCall;
    use near_kit::*;

    #[near_kit::contract]
    pub trait Configured {
        #[cfg(any())]
        fn missing_view(&self, args: DoesNotExist) -> u64;

        #[cfg(any())]
        #[call]
        fn missing_call(&mut self, args: DoesNotExist);

        #[cfg(test)]
        fn present_view(&self) -> u64;

        #[cfg(test)]
        #[call]
        fn present_call(&mut self);

        #[cfg_attr(test, deprecated = "use present_call")]
        #[call]
        fn old_call(&mut self);
    }

    #[test]
    fn compiles() {
        let near = Near::testnet().build();
        let client = near.contract::<Configured>(account!("configured.testnet"));
        let _view = client.present_view();
        let _call: FunctionCall = Configured::present_call();
        #[allow(deprecated)]
        let _old: FunctionCall = Configured::old_call();
    }
}

/// A format chosen by `cfg` is two declarations, each under its own `#[cfg]`
/// (a format marker behind `#[cfg_attr]` is rejected).
mod cfg_format_per_copy {
    use near_kit::*;

    #[near_kit::contract]
    pub trait Conditional {
        #[cfg(test)]
        #[borsh]
        fn get(&self) -> u64;

        #[cfg(not(test))]
        fn get(&self) -> u64;
    }

    #[test]
    fn compiles() {
        let near = Near::testnet().build();
        let _view = near
            .contract::<Conditional>(account!("conditional.testnet"))
            .get();
    }
}

/// `impl Trait` parameters are accepted, as in any trait method, named or
/// as the whole argument object.
mod impl_trait_args {
    use near_kit::Call;
    use near_kit::signer::{InMemorySigner, SecretKey};
    use near_kit::transaction::FunctionCall;
    use near_kit::*;

    #[near_kit::contract]
    pub trait Registry {
        fn lookup(&self, key: impl serde::Serialize) -> Option<String>;

        #[call]
        fn register(&mut self, #[args] args: impl serde::Serialize);
    }

    #[derive(serde::Serialize)]
    struct Key {
        key: String,
    }

    #[test]
    fn compiles() {
        let near = Near::testnet().build();
        let client = near.contract::<Registry>(account!("registry.testnet"));

        let _view = client.lookup("a");
        let account: SigningNear = near.with_signer(InMemorySigner::new(
            account!("alice.testnet"),
            SecretKey::generate_ed25519(),
        ));
        let signing_client = account.contract::<Registry>(account!("registry.testnet"));
        let _call: Call<SigningNear> = signing_client.register(Key { key: "a".into() });
        let _offline: FunctionCall = Registry::register(Key { key: "a".into() });
    }
}

/// A contract's own types can use names a generated generic might, such as
/// `C`.
mod user_type_named_c {
    use near_kit::*;
    use near_kit::{Call, View};

    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct C {
        value: u64,
    }

    #[near_kit::contract]
    pub trait Store {
        fn get(&self, #[args] args: C) -> C;

        #[call]
        fn set(&mut self, value: C);
    }

    #[test]
    fn compiles() {
        let near = Near::testnet().build();
        let client = near.contract::<Store>(account!("store.testnet"));
        let _view: View<Near, C> = client.get(C { value: 1 });
        let _call: fn(&StoreClient<SigningNear>, C) -> Call<SigningNear> = StoreClient::set;
    }
}

/// A generated client works over any `Viewer`/`TxSender`, not just `Near`,
/// and its futures stay `Send` for generic backends. Backends can also sit
/// behind `&`, `Box` and `Arc`, including as trait objects.
mod custom_backend {
    use std::sync::Arc;

    use near_kit::ContractError;
    use near_kit::protocol::Action;
    use near_kit::rpc::{BlockReference, ViewFunctionResult};
    use near_kit::*;

    #[near_kit::contract]
    pub trait Counter {
        fn get_count(&self) -> u64;

        #[call]
        fn increment(&mut self);
    }

    #[derive(Clone)]
    struct Mock(AccountId);

    impl Viewer for Mock {
        type ViewError = String;

        async fn view_function(
            &self,
            _contract_id: &AccountId,
            _method: &str,
            _args: &[u8],
            _block: BlockReference,
        ) -> Result<ViewFunctionResult, String> {
            Err("offline".to_owned())
        }
    }

    impl TxSender for Mock {
        type Output = ();
        type SendError = String;

        fn account_id(&self) -> &AccountId {
            &self.0
        }

        async fn send(&self, _receiver_id: AccountId, _actions: Vec<Action>) -> Result<(), String> {
            Ok(())
        }
    }

    async fn bump<C>(counter: &CounterClient<C>) -> Result<u64, ContractError<C::ViewError>>
    where
        C: Viewer + TxSender + Clone + 'static,
        C::ViewError: 'static,
        C::Output: 'static,
        C::SendError: 'static,
    {
        let _ = counter.increment().await;
        counter.get_count().await
    }

    /// A user's object-safe sender, used as `Arc<dyn UserSender>`.
    trait UserSender: Send + Sync {
        fn id(&self) -> &AccountId;
    }

    impl UserSender for Mock {
        fn id(&self) -> &AccountId {
            &self.0
        }
    }

    impl TxSender for dyn UserSender {
        type Output = ();
        type SendError = String;

        fn account_id(&self) -> &AccountId {
            self.id()
        }

        async fn send(&self, _receiver_id: AccountId, _actions: Vec<Action>) -> Result<(), String> {
            Ok(())
        }
    }

    fn assert_send<F: std::future::Future + Send>(_: F) {}

    #[test]
    fn compiles() {
        let counter: CounterClient<Mock> =
            Counter::client(Mock(account!("me.near")), account!("c.near"));
        assert_send(bump(&counter));

        let shared: Arc<dyn UserSender> = Arc::new(Mock(account!("me.near")));
        let counter = Counter::client(shared, account!("c.near"));
        assert_send(async move {
            let _ = counter.increment().await;
        });
    }
}
