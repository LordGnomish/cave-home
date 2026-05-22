// SPDX-License-Identifier: Apache-2.0
//! Service registry — port of `homeassistant/core.py::ServiceRegistry`.
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::ServiceRegistry

use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use futures::future::BoxFuture;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::Context;
use crate::error::{HassError, HassResult};
use crate::event_bus::{
    EVENT_CALL_SERVICE, EVENT_SERVICE_REGISTERED, EVENT_SERVICE_REMOVED, EventBus, EventOrigin,
};

/// Whether and how a service returns response data.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::SupportsResponse
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SupportsResponse {
    /// The service does not support responses (the default).
    #[default]
    None,
    /// The service optionally returns response data when asked.
    Optional,
    /// The service is read-only and the caller must always ask.
    Only,
}

/// Response value from a service handler.
pub type ServiceResponse = Option<Value>;

/// Boxed future returned by a service handler.
pub type ServiceFuture = BoxFuture<'static, HassResult<ServiceResponse>>;

/// Service handler — closure taking a [`ServiceCall`] and returning a
/// future of `Option<Value>`.
pub type ServiceHandler =
    Arc<dyn Fn(ServiceCall) -> ServiceFuture + Send + Sync + 'static>;

/// Optional service-call data schema. Called to coerce/validate the
/// payload before the handler runs.
pub type ServiceSchema =
    Arc<dyn Fn(&Value) -> HassResult<Value> + Send + Sync + 'static>;

/// Representation of a callable service.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::Service
#[derive(Clone)]
pub struct Service {
    pub domain: String,
    pub name: String,
    pub handler: ServiceHandler,
    pub schema: Option<ServiceSchema>,
    pub supports_response: SupportsResponse,
}

impl fmt::Debug for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Service")
            .field("domain", &self.domain)
            .field("name", &self.name)
            .field("supports_response", &self.supports_response)
            .field("has_schema", &self.schema.is_some())
            .finish()
    }
}

/// Representation of a call to a service.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::ServiceCall
#[derive(Debug, Clone, Serialize)]
pub struct ServiceCall {
    pub domain: String,
    pub service: String,
    pub data: Value,
    pub context: Context,
    pub return_response: bool,
}

/// Service registry — offers services over the event bus.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::ServiceRegistry
#[derive(Debug)]
pub struct ServiceRegistry {
    services: RwLock<HashMap<String, HashMap<String, Service>>>,
    bus: Arc<dyn EventBus>,
}

impl ServiceRegistry {
    #[must_use]
    pub fn new(bus: Arc<dyn EventBus>) -> Self {
        Self {
            services: RwLock::new(HashMap::new()),
            bus,
        }
    }

    /// Test if `domain.service` is registered.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::ServiceRegistry.has_service
    pub fn has_service(&self, domain: &str, service: &str) -> bool {
        let services = self.services.read();
        services
            .get(&domain.to_ascii_lowercase())
            .is_some_and(|m| m.contains_key(&service.to_ascii_lowercase()))
    }

    /// Register a new service.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::ServiceRegistry.async_register
    pub fn register(
        &self,
        domain: &str,
        service: &str,
        handler: ServiceHandler,
        schema: Option<ServiceSchema>,
        supports_response: SupportsResponse,
    ) {
        let domain = domain.to_ascii_lowercase();
        let service = service.to_ascii_lowercase();
        let svc = Service {
            domain: domain.clone(),
            name: service.clone(),
            handler,
            schema,
            supports_response,
        };
        {
            let mut services = self.services.write();
            services
                .entry(domain.clone())
                .or_default()
                .insert(service.clone(), svc);
        }
        self.bus.fire_parts(
            EVENT_SERVICE_REGISTERED,
            serde_json::json!({"domain": domain, "service": service}),
            EventOrigin::Local,
            Context::new(),
        );
    }

    /// Remove a registered service.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::ServiceRegistry.async_remove
    pub fn remove(&self, domain: &str, service: &str) {
        let domain = domain.to_ascii_lowercase();
        let service = service.to_ascii_lowercase();
        let removed = {
            let mut services = self.services.write();
            let removed = services
                .get_mut(&domain)
                .is_some_and(|m| m.remove(&service).is_some());
            if let Some(m) = services.get(&domain) {
                if m.is_empty() {
                    services.remove(&domain);
                }
            }
            removed
        };
        if removed {
            self.bus.fire_parts(
                EVENT_SERVICE_REMOVED,
                serde_json::json!({"domain": domain, "service": service}),
                EventOrigin::Local,
                Context::new(),
            );
        }
    }

    /// Return a snapshot of all registered services.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::ServiceRegistry.async_services
    pub fn services(&self) -> HashMap<String, HashMap<String, Service>> {
        self.services.read().clone()
    }

    /// Async-call a service.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::ServiceRegistry.async_call
    pub async fn call(
        &self,
        domain: &str,
        service: &str,
        data: Option<Value>,
        context: Option<Context>,
        return_response: bool,
    ) -> HassResult<ServiceResponse> {
        let domain_l = domain.to_ascii_lowercase();
        let service_l = service.to_ascii_lowercase();
        let handler = {
            let services = self.services.read();
            services
                .get(&domain_l)
                .and_then(|m| m.get(&service_l))
                .cloned()
        }
        .ok_or_else(|| HassError::ServiceNotFound {
            domain: domain_l.clone(),
            service: service_l.clone(),
        })?;

        if return_response && handler.supports_response == SupportsResponse::None {
            return Err(HassError::ServiceValidationError(format!(
                "{domain_l}.{service_l} does not support return_response=true"
            )));
        }
        if !return_response && handler.supports_response == SupportsResponse::Only {
            return Err(HassError::ServiceValidationError(format!(
                "{domain_l}.{service_l} only returns responses; pass return_response=true"
            )));
        }

        let raw = data.unwrap_or(Value::Object(serde_json::Map::new()));
        let validated = if let Some(schema) = handler.schema.as_ref() {
            schema(&raw)?
        } else {
            raw
        };
        let ctx = context.unwrap_or_default();
        let call = ServiceCall {
            domain: domain_l.clone(),
            service: service_l.clone(),
            data: validated.clone(),
            context: ctx.clone(),
            return_response,
        };

        self.bus.fire_parts(
            EVENT_CALL_SERVICE,
            serde_json::json!({
                "domain": domain_l,
                "service": service_l,
                "service_data": validated,
            }),
            EventOrigin::Local,
            ctx,
        );

        (handler.handler)(call).await
    }
}

/// Tiny helper for wrapping an async closure as a [`ServiceHandler`].
///
/// # Examples
/// ```
/// use cave_home_automation::service::{service_handler, ServiceCall};
/// use cave_home_automation::error::HassResult;
/// let h = service_handler(|_call: ServiceCall| async move {
///     Ok::<_, cave_home_automation::error::HassError>(None)
/// });
/// # let _ = h;
/// ```
#[allow(clippy::type_complexity)]
pub fn service_handler<F, Fut>(f: F) -> ServiceHandler
where
    F: Fn(ServiceCall) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = HassResult<ServiceResponse>> + Send + 'static,
{
    Arc::new(move |call: ServiceCall| -> ServiceFuture {
        let fut = f(call);
        Box::pin(fut) as Pin<Box<_>>
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::InMemoryEventBus;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make() -> Arc<ServiceRegistry> {
        Arc::new(ServiceRegistry::new(Arc::new(InMemoryEventBus::new())))
    }

    /// Upstream-test: `tests/test_core.py::test_service_registry_register_call`
    #[tokio::test]
    async fn register_and_call_runs_handler() {
        let reg = make();
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let handler = service_handler(move |_call| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        });
        reg.register("light", "turn_on", handler, None, SupportsResponse::None);
        assert!(reg.has_service("light", "turn_on"));
        reg.call("light", "turn_on", None, None, false).await.unwrap();
        reg.call("LIGHT", "TURN_ON", None, None, false).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    /// Upstream-test: `tests/test_core.py::test_service_registry_not_found`
    #[tokio::test]
    async fn call_unknown_service_errors() {
        let reg = make();
        let err = reg
            .call("ghost", "noop", None, None, false)
            .await
            .unwrap_err();
        assert!(matches!(err, HassError::ServiceNotFound { .. }));
    }

    /// Upstream-test: `tests/test_core.py::test_service_call_blocks_until_handler_returns`
    #[tokio::test]
    async fn blocking_call_waits_for_handler() {
        let reg = make();
        let handler = service_handler(|_call| async move {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            Ok(Some(Value::from(42)))
        });
        reg.register("test", "echo", handler, None, SupportsResponse::Only);
        let response = reg
            .call("test", "echo", None, None, true)
            .await
            .unwrap();
        assert_eq!(response, Some(Value::from(42)));
    }

    #[tokio::test]
    async fn schema_runs_before_handler() {
        let reg = make();
        let schema: ServiceSchema = Arc::new(|data: &Value| {
            if data.get("brightness").is_none() {
                return Err(HassError::ServiceValidationError(
                    "missing brightness".into(),
                ));
            }
            Ok(data.clone())
        });
        let handler = service_handler(|call| async move {
            Ok(Some(call.data.get("brightness").cloned().unwrap_or(Value::Null)))
        });
        reg.register(
            "light",
            "turn_on",
            handler,
            Some(schema),
            SupportsResponse::Optional,
        );
        let r = reg
            .call("light", "turn_on", Some(serde_json::json!({"brightness": 100})), None, true)
            .await
            .unwrap();
        assert_eq!(r, Some(Value::from(100)));
        let err = reg
            .call("light", "turn_on", Some(serde_json::json!({})), None, true)
            .await
            .unwrap_err();
        assert!(matches!(err, HassError::ServiceValidationError(_)));
    }

    #[tokio::test]
    async fn return_response_only_requires_request() {
        let reg = make();
        let handler = service_handler(|_call| async move { Ok(Some(Value::from(1))) });
        reg.register("test", "ro", handler, None, SupportsResponse::Only);
        let err = reg
            .call("test", "ro", None, None, false)
            .await
            .unwrap_err();
        assert!(matches!(err, HassError::ServiceValidationError(_)));
    }

    #[tokio::test]
    async fn remove_service_unregisters() {
        let reg = make();
        let handler = service_handler(|_call| async move { Ok(None) });
        reg.register("light", "turn_on", handler, None, SupportsResponse::None);
        assert!(reg.has_service("light", "turn_on"));
        reg.remove("light", "turn_on");
        assert!(!reg.has_service("light", "turn_on"));
    }
}
