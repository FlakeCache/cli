//! HTTP client abstraction and CBOR protocol implementation
//!
//! Provides a clean HTTP client interface for communicating with the FlakeCache server,
//! including CBOR serialization/deserialization for efficient binary protocol.

pub mod cbor;
pub mod request;
pub mod response;
