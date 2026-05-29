// SPDX-License-Identifier: Apache-2.0
//! OCI runtime-spec construction and the OCI version constant.

pub mod spec;
pub mod version;

pub use spec::{
    ContainerConfig, LinuxNamespace, LinuxSpec, Mount, NamespaceType, Process, Resources, Root,
    RuntimeSpec, SpecError, generate_spec,
};
pub use version::OCI_VERSION;
