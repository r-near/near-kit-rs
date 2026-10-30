//! Test: attributes on parameters are rejected, not silently dropped

#[near_kit::contract]
pub trait BadContract {
    fn get(&self, #[serde(rename = "accountId")] account_id: String, limit: u32) -> u64;
}

fn main() {}
