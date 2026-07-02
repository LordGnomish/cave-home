// SPDX-License-Identifier: Apache-2.0
//! gRPC `Status` -> `CriError` mapping.
//!
//! Mirrors how `pkg/kubelet/cri/remote` inspects the gRPC status code: a
//! NOT_FOUND becomes the typed not-found error the pod workers already branch
//! on, every other code is surfaced verbatim so callers can log/retry.
#![cfg(feature = "remote-cri")]

use cave_home_kubelet_rs::cri::remote::error::status_to_cri_error;
use cave_home_kubelet_rs::cri::CriError;
use tonic::{Code, Status};

#[test]
fn not_found_maps_to_typed_not_found() {
    let e = status_to_cri_error(&Status::not_found("pod sandbox sb-1"));
    assert!(matches!(e, CriError::NotFound(ref m) if m == "pod sandbox sb-1"));
}

#[test]
fn other_codes_map_to_rpc_preserving_code_and_message() {
    let e = status_to_cri_error(&Status::new(Code::Unavailable, "containerd down"));
    match e {
        CriError::Rpc { code, message } => {
            assert_eq!(code, Code::Unavailable as i32);
            assert_eq!(message, "containerd down");
        }
        other => panic!("expected Rpc, got {other:?}"),
    }
}

#[test]
fn invalid_argument_is_not_swallowed_as_not_found() {
    let e = status_to_cri_error(&Status::new(Code::InvalidArgument, "bad config"));
    assert!(matches!(e, CriError::Rpc { .. }));
}
