//! The POPCORN node: storage, collection, block production, API and replay verification.
//!
//! Nothing here can influence the state root — that lives entirely in `popcorn-core`. What
//! this crate decides is operational: when to close collection, how to defend availability
//! during a blind phase that collects no fees, and what to serve. §13.3 calls these changes
//! free, and they are: two nodes running different versions of this crate still produce the
//! same chain.

pub mod api;
pub mod chain;
pub mod client;
pub mod encoding;
pub mod keys;
pub mod mempool;
pub mod producer;
pub mod storage;
pub mod verify;
