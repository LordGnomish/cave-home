// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Build script: compile the vendored etcd proto into Rust via tonic-build,
//! but only when the `grpc` feature is enabled. Without `grpc` this is a no-op,
//! so the default (SQLite-only) build pulls in no protoc / codegen.

// Without the `grpc` feature there is nothing fallible to do, but the signature
// stays uniform across configs.
#[cfg_attr(not(feature = "grpc"), allow(clippy::unnecessary_wraps))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "grpc")]
    {
        println!("cargo:rerun-if-changed=proto/rpc.proto");
        tonic_build::configure()
            .build_server(true)
            .build_client(true)
            .compile_protos(&["proto/rpc.proto"], &["proto"])?;
    }
    Ok(())
}
