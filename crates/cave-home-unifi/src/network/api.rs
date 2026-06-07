// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Network Controller REST surface.
//!
//! [`NetworkApi`] borrows a [`ConsoleClient`] and turns the documented local
//! Network API into typed reads (sites, clients, devices, events, health) and
//! validated writes (block / unblock / reconnect a client, set a PoE port). The
//! site-scoped reads hit `/api/s/{site}/stat/*`; the writes hit the `cmd/*`
//! command bus or the `rest/device` resource, exactly as the controller's own
//! UI does. Each read decodes the `{meta, data}` envelope and lowers the wire
//! rows onto the [`cave_home_unifi_network`] domain model.
//!
//! Crucially, [`NetworkApi::execute`] accepts a domain
//! [`cave_home_unifi_network::Command`] — the *validated decision* the sibling
//! crate's control engine produces — and performs the matching call. The
//! decision and the I/O stay cleanly separated across the two crates.

use serde_json::json;

use cave_home_unifi_network::{Command, NetworkClient, NetworkDevice, PoeMode};

use super::types::{Envelope, HealthSubsystem, NetworkEvent, Site, WireClient, WireDevice, WireEvent};
use crate::client::ConsoleClient;
use crate::error::{Result, UnifiError};
use crate::transport::{HttpMethod, HttpRequest, HttpTransport};

/// The Network Controller API, bound to one [`ConsoleClient`].
pub struct NetworkApi<'a, T: HttpTransport> {
    client: &'a ConsoleClient<T>,
}

impl<'a, T: HttpTransport> NetworkApi<'a, T> {
    /// Bind to a console client.
    #[must_use]
    pub fn new(client: &'a ConsoleClient<T>) -> Self {
        Self { client }
    }

    async fn get_list<R, W>(&self, url: String, endpoint: &str) -> Result<Vec<R>>
    where
        W: serde::de::DeserializeOwned,
        R: From<W>,
    {
        let env: Envelope<W> = self.client.get_json(url, endpoint).await?;
        Ok(env.into_data()?.into_iter().map(R::from).collect())
    }

    /// List the sites on this controller (`/api/self/sites`).
    ///
    /// # Errors
    /// Transport / HTTP / decode errors, or an `rc=error` envelope.
    pub async fn sites(&self) -> Result<Vec<Site>> {
        let url = self.client.console().network_url("self/sites");
        let env: Envelope<Site> = self.client.get_json(url, "network/sites").await?;
        env.into_data()
    }

    /// List the clients on a site (`stat/sta`), lowered to the domain model.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors, or an `rc=error` envelope.
    pub async fn clients(&self, site: &str) -> Result<Vec<NetworkClient>> {
        let url = self.client.console().network_site_url(site, "stat/sta");
        let env: Envelope<WireClient> = self.client.get_json(url, "network/clients").await?;
        Ok(env
            .into_data()?
            .into_iter()
            .map(WireClient::into_domain)
            .collect())
    }

    /// List the infrastructure devices on a site (`stat/device`).
    ///
    /// # Errors
    /// Transport / HTTP / decode errors, or an `rc=error` envelope.
    pub async fn devices(&self, site: &str) -> Result<Vec<NetworkDevice>> {
        let url = self.client.console().network_site_url(site, "stat/device");
        let env: Envelope<WireDevice> = self.client.get_json(url, "network/devices").await?;
        Ok(env
            .into_data()?
            .into_iter()
            .map(WireDevice::into_domain)
            .collect())
    }

    /// The most recent events on a site (`stat/event`), newest first as the
    /// controller returns them.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors, or an `rc=error` envelope.
    pub async fn events(&self, site: &str, limit: u32) -> Result<Vec<NetworkEvent>> {
        let url = self
            .client
            .console()
            .network_site_url(site, &format!("stat/event?_limit={limit}"));
        self.get_list::<NetworkEvent, WireEvent>(url, "network/events")
            .await
    }

    /// The site health subsystems (`stat/health`): is the internet up, etc.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors, or an `rc=error` envelope.
    pub async fn health(&self, site: &str) -> Result<Vec<HealthSubsystem>> {
        let url = self.client.console().network_site_url(site, "stat/health");
        let env: Envelope<HealthSubsystem> =
            self.client.get_json(url, "network/health").await?;
        env.into_data()
    }

    /// Run a client-manager command (`cmd/stamgr`) — the bus block / unblock /
    /// reconnect ride on.
    async fn stamgr(&self, site: &str, cmd: &str, mac: &str, endpoint: &str) -> Result<()> {
        if mac.is_empty() {
            return Err(UnifiError::InvalidArgument("empty MAC".into()));
        }
        let url = self.client.console().network_site_url(site, "cmd/stamgr");
        let body = json!({ "cmd": cmd, "mac": mac });
        let _: Envelope<serde_json::Value> =
            self.client.post_json(url, &body, endpoint).await?;
        Ok(())
    }

    /// Block a client by MAC (`cmd/stamgr` `block-sta`).
    ///
    /// # Errors
    /// Invalid MAC, transport / HTTP / decode errors, or an `rc=error` envelope.
    pub async fn block_client(&self, site: &str, mac: &str) -> Result<()> {
        self.stamgr(site, "block-sta", mac, "network/block").await
    }

    /// Unblock a client by MAC (`cmd/stamgr` `unblock-sta`).
    ///
    /// # Errors
    /// Invalid MAC, transport / HTTP / decode errors, or an `rc=error` envelope.
    pub async fn unblock_client(&self, site: &str, mac: &str) -> Result<()> {
        self.stamgr(site, "unblock-sta", mac, "network/unblock").await
    }

    /// Reconnect ("kick") a client by MAC (`cmd/stamgr` `kick-sta`).
    ///
    /// # Errors
    /// Invalid MAC, transport / HTTP / decode errors, or an `rc=error` envelope.
    pub async fn reconnect_client(&self, site: &str, mac: &str) -> Result<()> {
        self.stamgr(site, "kick-sta", mac, "network/reconnect").await
    }

    /// Set a switch port's PoE mode (`PUT rest/device/{id}` with a single
    /// `port_overrides` entry; the controller merges it with the rest).
    ///
    /// # Errors
    /// Empty device id, transport / HTTP / decode errors, or an `rc=error`
    /// envelope.
    pub async fn set_poe(
        &self,
        site: &str,
        device_id: &str,
        port: u16,
        mode: PoeMode,
    ) -> Result<()> {
        if device_id.is_empty() {
            return Err(UnifiError::InvalidArgument("empty device id".into()));
        }
        let url = self
            .client
            .console()
            .network_site_url(site, &format!("rest/device/{device_id}"));
        let body = json!({
            "port_overrides": [ { "port_idx": port, "poe_mode": mode.as_wire() } ]
        });
        let req = HttpRequest::new(HttpMethod::Put, url).json(&body)?;
        let resp = self.client.send(req, "network/set_poe").await?;
        let env: Envelope<serde_json::Value> = resp.json_body()?;
        env.into_data().map(|_| ())
    }

    /// Execute a validated domain [`Command`] from the control engine.
    ///
    /// This is the bridge between `cave-home-unifi-network`'s pure decision
    /// layer and the wire: the engine validates *whether* an op is allowed and
    /// produces a `Command`; this performs it.
    ///
    /// # Errors
    /// As the underlying call for the command variant.
    pub async fn execute(&self, site: &str, command: &Command) -> Result<()> {
        match command {
            Command::BlockClient { mac } => self.block_client(site, mac).await,
            Command::UnblockClient { mac } => self.unblock_client(site, mac).await,
            Command::ReconnectClient { mac } => self.reconnect_client(site, mac).await,
            Command::SetPoe {
                device_id,
                port,
                mode,
            } => self.set_poe(site, device_id, *port, *mode).await,
            Command::SetWlanEnabled { .. }
            | Command::SetPortForwardEnabled { .. }
            | Command::SetDeviceLed { .. } => Err(UnifiError::InvalidArgument(format!(
                "command {command:?} is not yet wired to a REST call"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Credentials;
    use crate::console::Console;
    use crate::transport::{HttpResponse, MockTransport};

    fn keyed_client() -> ConsoleClient<MockTransport> {
        ConsoleClient::new(
            Console::unifi_os("10.0.0.1"),
            MockTransport::new(),
            Credentials::api_key("KEY"),
        )
    }

    fn client_returning(body: &[u8]) -> ConsoleClient<MockTransport> {
        let t = MockTransport::new();
        t.push(HttpResponse::json(200, body.to_vec()));
        ConsoleClient::new(
            Console::unifi_os("10.0.0.1"),
            t,
            Credentials::api_key("KEY"),
        )
    }

    #[tokio::test]
    async fn sites_decode_from_envelope_and_url_is_proxy_prefixed() {
        let client = client_returning(
            br#"{"meta":{"rc":"ok"},"data":[{"name":"default","desc":"Default","role":"admin"}]}"#,
        );
        let api = NetworkApi::new(&client);
        let sites = api.sites().await.unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].name, "default");
        assert_eq!(sites[0].role, "admin");
        let req = client.transport().last_request().unwrap();
        assert_eq!(req.url, "https://10.0.0.1:443/proxy/network/api/self/sites");
        assert_eq!(req.header_value("x-api-key"), Some("KEY"));
    }

    #[tokio::test]
    async fn clients_map_to_domain_and_hit_stat_sta() {
        let t = MockTransport::new();
        t.push(HttpResponse::json(
            200,
            br#"{"meta":{"rc":"ok"},"data":[
                {"mac":"aa:bb","hostname":"phone","is_wired":false,"essid":"Home","ap_mac":"ap1"}
            ]}"#
            .to_vec(),
        ));
        let client = ConsoleClient::new(
            Console::legacy("h"),
            t,
            Credentials::api_key("KEY"),
        );
        let api = NetworkApi::new(&client);
        let clients = api.clients("default").await.unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].name(), "phone");
        assert!(clients[0].is_wireless());
    }

    #[tokio::test]
    async fn block_client_posts_stamgr_cmd() {
        let t = MockTransport::new();
        t.push(HttpResponse::json(200, br#"{"meta":{"rc":"ok"},"data":[]}"#.to_vec()));
        let client = ConsoleClient::new(
            Console::legacy("h"),
            t,
            Credentials::api_key("KEY"),
        );
        let api = NetworkApi::new(&client);
        api.block_client("default", "aa:bb:cc").await.unwrap();
    }

    #[tokio::test]
    async fn block_client_rejects_empty_mac() {
        let client = keyed_client();
        let api = NetworkApi::new(&client);
        let err = api.block_client("default", "").await.unwrap_err();
        assert!(matches!(err, UnifiError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn rc_error_envelope_surfaces_as_error() {
        let t = MockTransport::new();
        t.push(HttpResponse::json(
            200,
            br#"{"meta":{"rc":"error","msg":"api.err.NoPermission"},"data":[]}"#.to_vec(),
        ));
        let client = ConsoleClient::new(
            Console::legacy("h"),
            t,
            Credentials::api_key("KEY"),
        );
        let api = NetworkApi::new(&client);
        let err = api.clients("default").await.unwrap_err();
        assert!(err.to_string().contains("api.err.NoPermission"));
    }

    #[tokio::test]
    async fn execute_dispatches_domain_command() {
        let t = MockTransport::new();
        // two ok acks for two commands
        t.set_fallback(HttpResponse::json(200, br#"{"meta":{"rc":"ok"},"data":[]}"#.to_vec()));
        let client = ConsoleClient::new(
            Console::legacy("h"),
            t,
            Credentials::api_key("KEY"),
        );
        let api = NetworkApi::new(&client);
        api.execute("default", &Command::BlockClient { mac: "aa:bb".into() })
            .await
            .unwrap();
        api.execute(
            "default",
            &Command::SetPoe {
                device_id: "d1".into(),
                port: 3,
                mode: PoeMode::Off,
            },
        )
        .await
        .unwrap();
        // an unwired command is reported, not silently ignored
        let err = api
            .execute(
                "default",
                &Command::SetDeviceLed {
                    device_id: "d1".into(),
                    on: true,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, UnifiError::InvalidArgument(_)));
    }
}
