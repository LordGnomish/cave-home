// SPDX-License-Identifier: Apache-2.0
//! Build script: compile the CRI v1 protobuf contract into Rust gRPC stubs.
//!
//! Codegen only runs when the `remote-cri` feature is enabled — the default
//! decision-core build needs neither protoc nor the generated transport code.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Cargo sets CARGO_FEATURE_<NAME> for every enabled feature.
    if std::env::var_os("CARGO_FEATURE_REMOTE_CRI").is_none() {
        return Ok(());
    }

    println!("cargo:rerun-if-changed=proto/api.proto");

    tonic_prost_build::configure()
        .build_client(true)
        // The in-process mock CRI server used by the integration tests needs
        // the server side too.
        .build_server(true)
        .compile_protos(&["proto/api.proto"], &["proto"])?;

    Ok(())
}
