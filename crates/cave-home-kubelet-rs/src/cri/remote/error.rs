// SPDX-License-Identifier: Apache-2.0
//! gRPC error translation for the remote CRI client.
//!
//! `pkg/kubelet/cri/remote` treats a `NOT_FOUND` from the runtime specially —
//! the pod workers branch on "does this sandbox/container still exist?" — while
//! every other status is an operational failure to be logged and retried. We
//! preserve that distinction by mapping onto [`CriError`].

use crate::cri::CriError;

/// Translate a gRPC [`tonic::Status`] into a [`CriError`].
///
/// `NOT_FOUND` becomes [`CriError::NotFound`]; any other code becomes
/// [`CriError::Rpc`] carrying the numeric code and message verbatim.
#[must_use]
pub fn status_to_cri_error(status: &tonic::Status) -> CriError {
    match status.code() {
        tonic::Code::NotFound => CriError::NotFound(status.message().to_owned()),
        code => CriError::Rpc {
            code: code as i32,
            message: status.message().to_owned(),
        },
    }
}

/// Translate a gRPC transport/connection error into [`CriError::Transport`].
#[must_use]
pub fn transport_to_cri_error(err: &tonic::transport::Error) -> CriError {
    CriError::Transport(err.to_string())
}
