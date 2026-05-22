# cave-home UI language

**Normative.** This is the source-of-truth translation matrix
between cave-home's technical implementation and the home-world
vocabulary the Portal + Mobile app are allowed to show end-users.
Required by **Charter §6.3** and **ADR-007** (grandma-friendly UX
mandate).

Any UI surface shipping a term not on this table — or surfacing a
"never show in UI" technical concept — fails review.

Last updated: 2026-05-14.

---

## Translation matrix

| Technical concept                          | How it appears in the UI                                                |
| ------------------------------------------ | ----------------------------------------------------------------------- |
| K3s cluster / node                         | "Ev Hub'ları" (Ana hub, Kamera hub, Ek hub)                             |
| Pod / container                            | "Eklenti" (Frigate eklentisi, sensör eklentisi)                         |
| Deployment / StatefulSet / DaemonSet       | *(invisible)* — UI shows only "Eklenti çalışıyor / durduruldu"          |
| MQTT topic / QoS / retain                  | *(invisible)* — "Cihaz konuşuyor / sessiz"                              |
| Zigbee PAN-ID / channel / network key      | *(invisible)* — "Zigbee ağı oluştur" button                             |
| Z-Wave network key / DSK                   | *(invisible)* — "Z-Wave ağı oluştur" button                             |
| Matter commissioning code                  | *(invisible — QR scan)* — "Matter cihazı ekle"                          |
| Modbus register / function code            | *(invisible)* — "Solar üretim: 4.2 kW"                                  |
| OBIS code                                  | *(invisible)* — "Bugün şebeke kullanımı: 12.4 kWh"                      |
| etcd / kine                                | *(invisible)* — "Yedek aktif, son yedek: 5 dk önce"                     |
| Helm chart                                 | *(invisible)* — "Eklenti mağazasından yükle"                            |
| Persona / RBAC role                        | "Aile rolü" (Yönetici / Üye / Misafir / Çocuk)                          |
| ConfigMap / Secret                         | *(invisible)* — "Cihaz ayarları" (one screen)                           |
| Manifest YAML                              | *(invisible)* — UI buttons + sliders                                    |
| LoadBalancer / Ingress                     | *(invisible)* — "Eve dışarıdan erişim aç / kapat" toggle                |
| Token / Certificate                        | *(invisible)* — abstracted as a **QR code** the user scans              |
| Join URL / cluster token                   | *(invisible)* — embedded in the QR code                                 |
| WireGuard peer / public key                | *(invisible)* — "Evden uzakta erişim" toggle                            |
| Container image registry                   | *(invisible)* — "Eklenti mağazası"                                      |
| Crash / OOMKilled / CrashLoopBackOff       | "Eklenti bir sorun yaşadı — yeniden başlatıyoruz" + automatic recovery  |
| kubelet / kube-proxy / apiserver           | *(invisible — never named)*                                             |
| CNI / pod network / VXLAN                  | *(invisible)* — "Eklentiler birbirleriyle konuşuyor" (status)           |
| Stack trace / log line / pod ID            | *(invisible in normal UI; visible in Developer view only)*              |

### Settings → "Developer view" toggle

Off by default. When the user enables it (Portal only — Mobile app
does **not** expose the toggle):

- Cluster topology page surfaces node names, K3s status, kine
  health, CNI status.
- Container logs become viewable per "Eklenti".
- Manifest editor (YAML) opens for power users.
- Raw MQTT traffic inspector.
- Anonymised debug bundle can be attached to a support / bug
  report.

Even with Developer view on, the **default routing** of the home-
world UI does not change. Developer view adds *additional* pages,
never relabels existing ones.

---

## Wording conventions

These conventions apply across all locales:

- **Devices over technology.** "Salon lambası" (Living-room lamp)
  is shown, not "Philips Hue White v2 RGB BT-mesh".
- **Outcomes over operations.** "Yedek aktif" (Backup active),
  not "etcd snapshot succeeded at 22:14:03 UTC".
- **People over roles.** "Aile" (Family), not "User group" or
  "RBAC subjects".
- **Rooms over hierarchies.** Rooms are first-class; "areas",
  "zones", "tags", and "groups" are implementation details
  underneath.
- **Status over state.** "Hub çevrimdışı" (Hub offline) for the
  end-user; "kubelet NotReady" only in Developer view.

## Error-message style

Every user-visible error must:

1. Name the symptom in a home-world term.
2. Suggest a concrete next step the user can try.
3. Avoid jargon the user did not produce.

Examples:

| ✅ Acceptable                                                  | ❌ Unacceptable                                                       |
| -------------------------------------------------------------- | --------------------------------------------------------------------- |
| "Hub'ın internete bağlanamadı. Wi-Fi şifreni kontrol et."     | "DNS resolution failed for upstream A record (NXDOMAIN)."             |
| "Cihaz 5 dakikadır cevap vermiyor. Pili değişebilir mi?"      | "Zigbee router lqi=12, last-seen 312s ago, retries=0."                |
| "Eklenti bir sorun yaşadı — yeniden başlatıyoruz."             | "CrashLoopBackOff: container 'frigate-detector' restart count = 7."   |

## Locales (i18n mandate, M1)

- **Turkish (`tr-TR`)** — primary developer locale; vocabulary
  examples in this document are TR-anchored.
- **English (`en-US`)** — required from M1 for the OSS launch.
- **German (`de-DE`)** — required from M1; Burak's home (Iphofen
  / Germany) is mixed-language, and the headline persona is
  intolerant of an English-only first release in DE-speaking
  households.

Future locales (FR, ES, NL, …) follow community demand and the
same normative table.

## How to update this file

1. Identify the new technical concept that needs UI surfacing
   (or hiding).
2. Add a row to the **Translation matrix** with:
   - Technical concept.
   - Home-world rendering, or `*(invisible)*` + an outcome
     statement that the UI does show instead.
3. If a new home-world term is introduced, add its `tr-TR /
   en-US / de-DE` triplet to the locale catalogues.
4. Land all three with a single PR. Reviewers reject any
   half-translated rows.
