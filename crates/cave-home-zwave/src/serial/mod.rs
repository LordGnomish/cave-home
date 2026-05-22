// SPDX-License-Identifier: Apache-2.0
//! Z-Wave Serial API frame layer.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/serial/src/
//!
//! - `parsers/SerialAPIParser.ts`  -> [`parser::SerialApiParser`]
//! - `message/Message.ts`          -> [`message::Message`] (`MessageRaw` + envelope)
//! - `message/Constants.ts`        -> [`constants::MessageType`] / [`constants::FunctionType`]
//! - `message/MessageHeaders.ts`   -> [`constants::MessageHeader`]
//!
//! Z-Wave's serial framing layer (INS12350) sits between USB UART bytes and
//! the controller-/node-level commands. This module owns the byte-accurate
//! port: framing, checksum, and the streaming parser used by [`crate::driver`].

pub mod constants;
pub mod message;
pub mod parser;

pub use constants::{FunctionType, MessageHeader, MessageType};
pub use message::{Message, MessageRaw, compute_checksum};
pub use parser::{SerialApiChunk, SerialApiParser};
