// SPDX-License-Identifier: Apache-2.0
//! Real CRI v1 gRPC client — the transport that talks to containerd.
//!
//! This module is the line-by-line port target of
//! `k8s.io/kubernetes/pkg/kubelet/cri/remote/`: it implements the crate's
//! [`CriClient`](crate::cri::CriClient) trait by driving the generated
//! `RuntimeServiceClient` / `ImageServiceClient` gRPC stubs over an HTTP/2
//! channel (TCP or a containerd Unix socket).
//!
//! It is compiled only under the `remote-cri` cargo feature so the kubelet
//! decision core stays free of the async-gRPC dependency stack.

/// Generated protobuf messages + tonic service stubs for `runtime.v1`.
///
/// Produced by `build.rs` from `proto/api.proto` (a verbatim copy of the
/// upstream CRI v1 contract). Generated code is exempt from the workspace
/// lint profile.
#[allow(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction,
    missing_docs,
    unreachable_pub,
    rustdoc::all
)]
pub mod proto {
    tonic::include_proto!("runtime.v1");
}

mod conv;
pub mod error;
