// SPDX-License-Identifier: Apache-2.0
//! Validation of Services / EndpointSlices before they enter the decision core.
//!
//! kube-proxy is tolerant of malformed objects (it logs and skips rather than
//! crashing), so the core rejects bad input with a typed error instead of
//! panicking. These checks mirror the documented Service/Endpoint API
//! invariants — they are intentionally minimal and side-effect-free.

use thiserror::Error;

use crate::core::model::{Endpoint, EndpointSlice, Service, ServicePort, ServiceType};

/// Why a Service / EndpointSlice was rejected by the decision core.
#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum ValidationError {
    #[error("service namespace/name must be non-empty")]
    EmptyServiceIdentity,
    #[error("service {key} declares no ports")]
    NoPorts { key: String },
    #[error("service {key} port {port_name:?} has invalid port {port}")]
    InvalidServicePort {
        key: String,
        port_name: String,
        port: u16,
    },
    #[error("service {key} port {port_name:?} has invalid targetPort {target}")]
    InvalidTargetPort {
        key: String,
        port_name: String,
        target: u16,
    },
    #[error("service {key} is type {kind} but no nodePort set on port {port_name:?}")]
    MissingNodePort {
        key: String,
        kind: &'static str,
        port_name: String,
    },
    #[error("service {key} port names are not unique: {dup:?}")]
    DuplicatePortName { key: String, dup: String },
    #[error("service {key} is LoadBalancer but has no ingress IP")]
    LoadBalancerWithoutIngress { key: String },
    #[error("endpointslice {ns}/{slice} references empty service-name")]
    SliceMissingService { ns: String, slice: String },
    #[error("endpointslice {ns}/{slice} has an endpoint with no addresses")]
    EndpointWithoutAddress { ns: String, slice: String },
    #[error("endpointslice {ns}/{slice} port {port_name:?} has invalid port {port}")]
    InvalidEndpointPort {
        ns: String,
        slice: String,
        port_name: String,
        port: u16,
    },
}

const fn kind_str(t: ServiceType) -> &'static str {
    match t {
        ServiceType::ClusterIp => "ClusterIP",
        ServiceType::NodePort => "NodePort",
        ServiceType::LoadBalancer => "LoadBalancer",
        ServiceType::ExternalName => "ExternalName",
    }
}

fn validate_port(key: &str, p: &ServicePort, ty: ServiceType) -> Result<(), ValidationError> {
    if p.port == 0 {
        return Err(ValidationError::InvalidServicePort {
            key: key.to_owned(),
            port_name: p.name.clone(),
            port: p.port,
        });
    }
    if p.target_port == 0 {
        return Err(ValidationError::InvalidTargetPort {
            key: key.to_owned(),
            port_name: p.name.clone(),
            target: p.target_port,
        });
    }
    if matches!(ty, ServiceType::NodePort | ServiceType::LoadBalancer)
        && p.node_port.is_none_or(|np| np == 0)
    {
        return Err(ValidationError::MissingNodePort {
            key: key.to_owned(),
            kind: kind_str(ty),
            port_name: p.name.clone(),
        });
    }
    Ok(())
}

/// Validate a Service. `Ok(())` means it is structurally sound enough to
/// program (a *skipped* Service — headless / ExternalName — still validates
/// fine; skipping is a separate decision in the rule builder).
///
/// # Errors
/// Returns the first [`ValidationError`] encountered.
pub fn validate_service(svc: &Service) -> Result<(), ValidationError> {
    if svc.namespace.is_empty() || svc.name.is_empty() {
        return Err(ValidationError::EmptyServiceIdentity);
    }
    let key = svc.key();

    // ExternalName services carry no ports/clusterIP; nothing else to check.
    if matches!(svc.service_type, ServiceType::ExternalName) {
        return Ok(());
    }

    if svc.ports.is_empty() {
        return Err(ValidationError::NoPorts { key });
    }

    let mut seen = std::collections::BTreeSet::new();
    for p in &svc.ports {
        if !p.name.is_empty() && !seen.insert(p.name.clone()) {
            return Err(ValidationError::DuplicatePortName {
                key,
                dup: p.name.clone(),
            });
        }
        validate_port(&key, p, svc.service_type)?;
    }

    if matches!(svc.service_type, ServiceType::LoadBalancer) && svc.load_balancer_ips.is_empty() {
        return Err(ValidationError::LoadBalancerWithoutIngress { key });
    }

    Ok(())
}

fn validate_endpoint(ns: &str, slice: &str, e: &Endpoint) -> Result<(), ValidationError> {
    if e.addresses.is_empty() {
        return Err(ValidationError::EndpointWithoutAddress {
            ns: ns.to_owned(),
            slice: slice.to_owned(),
        });
    }
    Ok(())
}

/// Validate an [`EndpointSlice`].
///
/// # Errors
/// Returns the first [`ValidationError`] encountered.
pub fn validate_slice(slice: &EndpointSlice) -> Result<(), ValidationError> {
    if slice.service_name.is_empty() {
        return Err(ValidationError::SliceMissingService {
            ns: slice.namespace.clone(),
            slice: slice.slice_name.clone(),
        });
    }
    for p in &slice.ports {
        if p.port == 0 {
            return Err(ValidationError::InvalidEndpointPort {
                ns: slice.namespace.clone(),
                slice: slice.slice_name.clone(),
                port_name: p.name.clone(),
                port: p.port,
            });
        }
    }
    for e in &slice.endpoints {
        validate_endpoint(&slice.namespace, &slice.slice_name, e)?;
    }
    Ok(())
}
