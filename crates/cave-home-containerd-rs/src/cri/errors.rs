// SPDX-License-Identifier: Apache-2.0
//! CRI server error taxonomy.
//!
//! Mirrors containerd's `errdefs` (used as `errdefs.ErrNotFound` etc.)
//! and additionally maps to `tonic::Status` codes for gRPC responses.

use thiserror::Error;
use tonic::Status;

/// Errors raised by the CRI in-memory stores and service handlers.
#[derive(Debug, Error)]
pub enum CriError {
    /// Resource (sandbox, container, image, …) not found.
    #[error("{0}: not found")]
    NotFound(String),
    /// Resource already exists.
    #[error("{0}: already exists")]
    AlreadyExists(String),
    /// Caller-supplied argument failed validation.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// Operation attempted in the wrong state (e.g. RemoveContainer
    /// on a Running container).
    #[error("failed precondition: {0}")]
    FailedPrecondition(String),
    /// Backed by content/snapshot/image error.
    #[error("internal: {0}")]
    Internal(String),
}

impl From<CriError> for Status {
    fn from(e: CriError) -> Self {
        match e {
            CriError::NotFound(m) => Self::not_found(m),
            CriError::AlreadyExists(m) => Self::already_exists(m),
            CriError::InvalidArgument(m) => Self::invalid_argument(m),
            CriError::FailedPrecondition(m) => Self::failed_precondition(m),
            CriError::Internal(m) => Self::internal(m),
        }
    }
}
