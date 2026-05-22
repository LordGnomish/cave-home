// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! UPnP SSDP discovery surface.
//!
//! The bridge participates in SSDP M-SEARCH (UDP 1900) and serves a
//! `description.xml` over HTTP. The payload is the published Hue Bridge
//! `<root>` document, with substitutions for our emulator identity.
//!
//! Reference: developer-portal "Hue Bridge discovery" → SSDP section.
//! Required headers (per UPnP 1.0 + Hue dev-portal docs):
//!   - `LOCATION: http://<bridge-ip>:80/description.xml`
//!   - `EXT:` (empty per UPnP spec)
//!   - `SERVER: Linux/<kernel> UPnP/1.0 IpBridge/<bridge-sw-version>`
//!   - `USN: uuid:<udn>::upnp:rootdevice`
//!   - `ST: upnp:rootdevice` (also `urn:schemas-upnp-org:device:Basic:1`)
//!   - `hue-bridgeid: <bridge-id>` (the Hue-specific extension)

use crate::config::BridgeIdentity;

/// Build the M-SEARCH response payload for an `ST: upnp:rootdevice` query.
/// Reference: SSDP + Hue developer-portal discovery docs.
#[must_use]
pub fn build_ssdp_response_root(identity: &BridgeIdentity) -> String {
    format!(
        "HTTP/1.1 200 OK\r\n\
         HOST: 239.255.255.250:1900\r\n\
         EXT:\r\n\
         CACHE-CONTROL: max-age=100\r\n\
         LOCATION: http://{host}:{port}/description.xml\r\n\
         SERVER: cave-home/0.0.0 UPnP/1.0 IpBridge/{sw}\r\n\
         hue-bridgeid: {bridge_id}\r\n\
         ST: upnp:rootdevice\r\n\
         USN: {udn}::upnp:rootdevice\r\n\r\n",
        host = identity.host,
        port = identity.http_port,
        sw = identity.software_version,
        bridge_id = identity.bridge_id.to_uppercase(),
        udn = identity.ssdp_udn(),
    )
}

/// Render the `description.xml` body served at `/description.xml`.
/// Reference: developer-portal "description.xml" example.
#[must_use]
pub fn build_description_xml(identity: &BridgeIdentity) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" ?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
<specVersion>
<major>1</major>
<minor>0</minor>
</specVersion>
<URLBase>http://{host}:{port}/</URLBase>
<device>
<deviceType>urn:schemas-upnp-org:device:Basic:1</deviceType>
<friendlyName>{name} ({host})</friendlyName>
<manufacturer>{manufacturer}</manufacturer>
<manufacturerURL>http://www.philips-hue.com</manufacturerURL>
<modelDescription>Philips hue Personal Wireless Lighting</modelDescription>
<modelName>{product}</modelName>
<modelNumber>{model}</modelNumber>
<modelURL>http://www.meethue.com</modelURL>
<serialNumber>{bridge_id}</serialNumber>
<UDN>{udn}</UDN>
<presentationURL>index.html</presentationURL>
<iconList>
<icon>
<mimetype>image/png</mimetype>
<height>48</height>
<width>48</width>
<depth>24</depth>
<url>hue_logo_0.png</url>
</icon>
</iconList>
</device>
</root>
"#,
        host = identity.host,
        port = identity.http_port,
        name = identity.name,
        manufacturer = identity.manufacturer_name,
        product = identity.product_name,
        model = identity.model_id,
        bridge_id = identity.bridge_id,
        udn = identity.ssdp_udn(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssdp_response_includes_required_headers() {
        let id = BridgeIdentity::fresh("10.0.0.5");
        let resp = build_ssdp_response_root(&id);
        // Per Hue dev-portal discovery docs, these headers are required.
        for needle in [
            "HTTP/1.1 200 OK",
            "LOCATION: http://10.0.0.5:80/description.xml",
            "SERVER:",
            "USN:",
            "ST: upnp:rootdevice",
            "hue-bridgeid:",
        ] {
            assert!(
                resp.contains(needle),
                "missing required header: {needle} in {resp}"
            );
        }
        // The bridge-id header carries the uppercase form per docs.
        assert!(resp.contains(&id.bridge_id.to_uppercase()));
    }

    #[test]
    fn description_xml_includes_uuid_and_serial() {
        let id = BridgeIdentity::fresh("10.0.0.5");
        let xml = build_description_xml(&id);
        assert!(xml.contains(&id.bridge_id));
        assert!(xml.contains(&id.ssdp_udn()));
        assert!(xml.contains("<modelName>Philips hue</modelName>"));
        assert!(xml.contains("<modelNumber>BSB002</modelNumber>"));
    }

    #[test]
    fn description_xml_has_required_friendly_name() {
        let id = BridgeIdentity::fresh("10.0.0.5");
        let xml = build_description_xml(&id);
        assert!(xml.contains("<friendlyName>"));
        // Friendly name embeds host per Hue dev-portal convention.
        assert!(xml.contains("(10.0.0.5)"));
    }
}
