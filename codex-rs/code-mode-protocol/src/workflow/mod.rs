//! Bounded native Workflow transport shared by Core and the isolated V8 host.
mod codec;
mod decoding;
mod status;
pub use codec::*;
pub use status::*;
#[cfg(test)]
mod native_contract_tests;
