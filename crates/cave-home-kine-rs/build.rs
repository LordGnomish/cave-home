// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Build script: compile the vendored etcd proto into Rust via tonic-build,
//! but only when the `grpc` feature is enabled. Without `grpc` this is a no-op,
//! so the default (SQLite-only) build pulls in no protoc / codegen.

fn main() {
    #[cfg(feature = "grpc")]
    {
        println!("cargo:rerun-if-changed=proto/rpc.proto");
        tonic_build::configure()
            .build_server(true)
            .build_client(true)
            .compile_protos(&["proto/rpc.proto"], &["proto"])
            .expect("failed to compile etcd proto");
    }
}
