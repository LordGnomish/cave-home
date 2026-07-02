// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Powerwall local-gateway client.

use serde_json::json;

use super::types::{MetersAggregates, SystemSoe};
use crate::error::{Result, TeslaError};
use crate::fleet_api::client::{HttpRequest, HttpResponse, HttpTransport};
use crate::fleet_api::endpoints::HttpMethod;
use crate::models::PowerFlowData;

/// The default local-gateway login user for a customer-owned Powerwall.
pub const DEFAULT_USERNAME: &str = "customer";

/// A local-gateway login request.
#[derive(Debug, Clone)]
pub struct LoginRequest {
    /// The login user (`customer` or `installer`).
    pub username: String,
    /// The gateway password (the last 5 chars of the serial, by default).
    pub password: String,
    /// The associated email, if the gateway requires it.
    pub email: Option<String>,
}

impl LoginRequest {
    /// A customer login with the given password.
    #[must_use]
    pub fn installer_default(password: impl Into<String>) -> Self {
        Self {
            username: DEFAULT_USERNAME.to_string(),
            password: password.into(),
            email: None,
        }
    }
}

/// Build the JSON body for `POST /api/login/Basic`.
#[must_use]
pub fn login_body(req: &LoginRequest) -> String {
    json!({
        "username": req.username,
        "password": req.password,
        "email": req.email.clone().unwrap_or_default(),
        "force_sm_off": false,
    })
    .to_string()
}

/// A client for one Powerwall's local gateway.
#[derive(Debug)]
pub struct GatewayClient<T: HttpTransport> {
    transport: T,
    base_url: String,
    token: Option<String>,
}

impl<T: HttpTransport> GatewayClient<T> {
    /// A client against `base_url` (e.g. `https://192.168.1.10`).
    #[must_use]
    pub fn new(transport: T, base_url: impl Into<String>) -> Self {
        Self {
            transport,
            base_url: base_url.into(),
            token: None,
        }
    }

    /// Attach an auth token obtained from `/api/login/Basic`.
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Borrow the transport (tests inspect requests through this).
    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    async fn get_json(&self, path: &str) -> Result<String> {
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if let Some(token) = &self.token {
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }
        let resp: HttpResponse = self
            .transport
            .send(HttpRequest {
                method: HttpMethod::Get,
                url: format!("{}{path}", self.base_url),
                headers,
                body: None,
            })
            .await?;
        if (200..300).contains(&resp.status) {
            Ok(resp.body)
        } else {
            Err(TeslaError::from_status(resp.status, &resp.body))
        }
    }

    /// Read the meter aggregates.
    ///
    /// # Errors
    /// A transport error, an error status, or a decode failure.
    pub async fn get_aggregates(&self) -> Result<MetersAggregates> {
        let body = self.get_json("/api/meters/aggregates").await?;
        Ok(serde_json::from_str(&body)?)
    }

    /// Read the aggregate state of energy, percent.
    ///
    /// # Errors
    /// A transport error, an error status, or a decode failure.
    pub async fn get_soe(&self) -> Result<f64> {
        let body = self.get_json("/api/system_status/soe").await?;
        let soe: SystemSoe = serde_json::from_str(&body)?;
        Ok(soe.percentage)
    }

    /// The instantaneous power flow, combining aggregates with the `SoE`.
    ///
    /// # Errors
    /// A transport error, an error status, or a decode failure on either read.
    pub async fn get_power_flow(&self) -> Result<PowerFlowData> {
        let aggregates = self.get_aggregates().await?;
        let soc = self.get_soe().await?;
        Ok(aggregates.power_flow(soc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet_api::client::MockTransport;

    #[test]
    fn login_body_carries_credentials() {
        let body = login_body(&LoginRequest::installer_default("secret"));
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["username"], "customer");
        assert_eq!(v["password"], "secret");
    }

    #[tokio::test]
    async fn get_power_flow_combines_aggregates_and_soe() {
        let t = MockTransport::new()
            .route(
                "/api/meters/aggregates",
                200,
                r#"{"site":{"instant_power":1200},"battery":{"instant_power":-1000},
                    "load":{"instant_power":2200},"solar":{"instant_power":2000}}"#,
            )
            .route("/api/system_status/soe", 200, r#"{"percentage":56.5}"#);
        let gw = GatewayClient::new(t, "https://192.168.1.10").with_token("tok");
        let f = gw.get_power_flow().await.unwrap();
        assert!((f.pv_watts - 2000.0).abs() < f64::EPSILON);
        assert!((f.soc_percent - 56.5).abs() < f64::EPSILON);
        assert!(f.battery_charging());
    }

    #[tokio::test]
    async fn get_power_flow_maps_error_status() {
        let t = MockTransport::new()
            .route("/api/meters/aggregates", 502, "bad gateway")
            .route("/api/system_status/soe", 200, r#"{"percentage":50}"#);
        let gw = GatewayClient::new(t, "https://192.168.1.10").with_token("tok");
        assert!(gw.get_power_flow().await.is_err());
    }

    #[tokio::test]
    async fn requests_target_the_gateway_base() {
        let t = MockTransport::new()
            .route("/api/meters/aggregates", 200, r#"{"site":{"instant_power":0},
                "battery":{"instant_power":0},"load":{"instant_power":0},"solar":{"instant_power":0}}"#)
            .route("/api/system_status/soe", 200, r#"{"percentage":0}"#);
        let gw = GatewayClient::new(t, "https://powerwall.local").with_token("tok");
        gw.get_power_flow().await.unwrap();
        let reqs = gw.transport().requests.lock();
        assert!(reqs.iter().all(|r| r.url.starts_with("https://powerwall.local")));
    }
}
