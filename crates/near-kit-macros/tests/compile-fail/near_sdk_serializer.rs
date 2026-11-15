//! Test: near-sdk's `#[serializer(..)]` is rejected on the method (it used
//! to be dropped) and on a parameter, where near-sdk puts it

#[near_kit::contract]
pub trait OnTheMethod {
    #[serializer(borsh)]
    #[call]
    fn set(&mut self, value: u64);
}

#[near_kit::contract]
pub trait OnTheParameter {
    #[call]
    fn set(&mut self, #[serializer(borsh)] value: u64);
}

fn main() {}
