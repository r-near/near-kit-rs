//! The polling loop from the finality guide ("Polling a view"): a cloned
//! `View`, `fetch_raw` each poll, and `decode` only when the bytes change and
//! the answer isn't from a block behind the highest one seen.
#![cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use near_kit::rpc::{BlockReference, ViewFunctionResult};
use near_kit::{AccountId, ContractError, CryptoHash, View, Viewer, account};
use serde::{Deserialize, Deserializer};

thread_local! {
    /// How many `Counted` values this test's thread has decoded. Each test
    /// runs on its own thread, and a current-thread runtime decodes there.
    static DECODES: Cell<usize> = const { Cell::new(0) };
}

fn decodes() -> usize {
    DECODES.with(Cell::get)
}

/// A `u64` that counts how often it is decoded.
#[derive(Debug, PartialEq)]
struct Counted(u64);

impl<'de> Deserialize<'de> for Counted {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        DECODES.with(|count| count.set(count.get() + 1));
        u64::deserialize(deserializer).map(Counted)
    }
}

/// Answers each view with the next `(bytes, block height)` from a script.
#[derive(Clone)]
struct Scripted(Arc<Mutex<VecDeque<(&'static str, u64)>>>);

impl Scripted {
    fn new(answers: impl IntoIterator<Item = (&'static str, u64)>) -> Self {
        Self(Arc::new(Mutex::new(answers.into_iter().collect())))
    }

    fn left(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

impl Viewer for Scripted {
    type ViewError = String;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        _method: &str,
        _args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, String> {
        let (bytes, height) = self.0.lock().unwrap().pop_front().expect("script ran out");
        Ok(ViewFunctionResult::new(
            bytes.as_bytes().to_vec(),
            height,
            CryptoHash::ZERO,
        ))
    }
}

/// The guide's loop, `polls` times and without the sleep: returns each
/// decoded value with the block it was first seen at.
async fn poll(
    view: View<Scripted, Counted>,
    polls: usize,
) -> Result<Vec<(u64, u64)>, ContractError<String>> {
    let mut seen = Vec::new();
    let mut last: Option<Vec<u8>> = None;
    let mut highest = 0;
    for _ in 0..polls {
        let raw = view.clone().fetch_raw().await?;
        if raw.block_height >= highest && last.as_ref() != Some(&raw.result) {
            let value = view.decode(&raw.result).map_err(ContractError::Codec)?;
            seen.push((value.0, raw.block_height));
            last = Some(raw.result);
        }
        highest = highest.max(raw.block_height);
    }
    Ok(seen)
}

fn view(viewer: &Scripted) -> View<Scripted, Counted> {
    View::json(viewer.clone(), account!("counter.near"), "get_count")
}

#[tokio::test(flavor = "current_thread")]
async fn decodes_once_per_distinct_result() {
    let viewer = Scripted::new([
        ("1", 10),
        ("1", 11),
        ("1", 12),
        ("2", 13),
        ("2", 14),
        ("3", 15),
        ("3", 16),
    ]);
    let seen = poll(view(&viewer), 7).await.unwrap();
    assert_eq!(viewer.left(), 0, "every poll ran the view");
    assert_eq!(seen, [(1, 10), (2, 13), (3, 15)]);
    assert_eq!(decodes(), 3, "one decode per distinct result, not per poll");
}

#[tokio::test(flavor = "current_thread")]
async fn ignores_an_answer_from_a_lower_block() {
    // A node behind the others answers with the value from before block 13.
    let viewer = Scripted::new([("1", 10), ("2", 13), ("1", 12), ("2", 14), ("1", 11)]);
    let seen = poll(view(&viewer), 5).await.unwrap();
    assert_eq!(seen, [(1, 10), (2, 13)], "the stale 1 is not news");
    assert_eq!(decodes(), 2, "nor is it decoded");
}

#[tokio::test(flavor = "current_thread")]
async fn awaiting_each_poll_decodes_every_time() {
    let viewer = Scripted::new([("1", 10), ("1", 11), ("1", 12)]);
    let view = view(&viewer);
    for _ in 0..3 {
        assert_eq!(view.clone().await.unwrap(), Counted(1));
    }
    assert_eq!(decodes(), 3);
}
