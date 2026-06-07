// SPDX-License-Identifier: Apache-2.0
//! Request types for the CRI streaming RPCs (`Exec`, `Attach`, `PortForward`).
//!
//! These mirror the protobuf requests. The RPCs themselves only *negotiate* a
//! streaming URL; the kubelet then opens a separate SPDY/WebSocket connection
//! to that URL to move bytes. cave-home implements the negotiation here; the
//! byte-streaming dialer is deferred (see the crate handoff / parity manifest).

/// Parameters for [`RemoteCriClient::exec`](super::RemoteCriClient::exec).
// The stdin/stdout/stderr/tty quartet mirrors the CRI `ExecRequest` proto
// 1:1; collapsing them would diverge from the wire contract.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecRequest {
    /// Container to run the command in.
    pub container_id: String,
    /// Command + args to execute.
    pub cmd: Vec<String>,
    /// Allocate a TTY.
    pub tty: bool,
    /// Stream stdin to the command.
    pub stdin: bool,
    /// Stream stdout from the command.
    pub stdout: bool,
    /// Stream stderr from the command.
    pub stderr: bool,
}

/// Parameters for [`RemoteCriClient::attach`](super::RemoteCriClient::attach).
// Mirrors the CRI `AttachRequest` proto's stdin/stdout/stderr/tty quartet.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AttachRequest {
    /// Container to attach to.
    pub container_id: String,
    /// Stream stdin to the container.
    pub stdin: bool,
    /// Allocate a TTY.
    pub tty: bool,
    /// Stream stdout from the container.
    pub stdout: bool,
    /// Stream stderr from the container.
    pub stderr: bool,
}

/// Parameters for
/// [`RemoteCriClient::port_forward`](super::RemoteCriClient::port_forward).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PortForwardRequest {
    /// Sandbox whose network namespace to forward into.
    pub pod_sandbox_id: String,
    /// Container ports to forward.
    pub ports: Vec<i32>,
}
