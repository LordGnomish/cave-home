// SPDX-License-Identifier: Apache-2.0
//! Authentication and authorization.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! - staging/src/k8s.io/apiserver/pkg/authentication/
//! - staging/src/k8s.io/apiserver/pkg/authorization/

pub mod authn;
pub mod authz;

pub use authn::{
    Authenticator, AuthnError, AuthnResult, ChainAuthenticator, ClientCertAuthenticator,
    ServiceAccountTokenAuthenticator,
};
pub use authz::{
    Authorizer, AuthzDecision, AuthzError, AuthzResult, RbacAuthorizer, Rule, RuleSet,
};
