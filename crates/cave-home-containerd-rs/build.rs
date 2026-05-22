// SPDX-License-Identifier: Apache-2.0
//
// Build script — generates Rust types and gRPC service traits from the
// vendored CRI v1 protobuf definition.
//
// Upstream proto: github.com/containerd/containerd@v2.3.0
//   path: vendor/k8s.io/cri-api/pkg/apis/runtime/v1/api.proto
//
// We use tonic-build, which wraps prost-build, to emit the generated code
// into OUT_DIR; consumers `tonic::include_proto!("runtime.v1")` to pull it in.

use std::io::Result;

fn main() -> Result<()> {
    let proto = "proto/runtime/v1/api.proto";
    println!("cargo:rerun-if-changed={proto}");
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&[proto], &["proto"])?;
    Ok(())
}
