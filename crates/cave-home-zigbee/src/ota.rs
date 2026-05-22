// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! OTA Upgrade cluster (0x0019) — ZCL §11.
//!
//! Phase 1 ships the queue, the [`OtaImageProvider`] trait that callers
//! plug in to source firmware images, and a signal handler that maps
//! ZCL OTA `Query Next Image Request` (0x01) into a queue entry. The
//! actual block transfer and signature verification land in Phase 1b.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::error::Result;

/// OTA cluster identifier — ZCL §11.2.
pub const OTA_CLUSTER_ID: u16 = 0x0019;

/// OTA command IDs (subset Phase 1 understands).
pub mod command_id {
    /// 0x01 — Query Next Image Request (client → server).
    pub const QUERY_NEXT_IMAGE_REQUEST: u8 = 0x01;
    /// 0x05 — Image Block Request (client → server).
    pub const IMAGE_BLOCK_REQUEST: u8 = 0x05;
    /// 0x06 — Upgrade End Request (client → server).
    pub const UPGRADE_END_REQUEST: u8 = 0x06;
}

/// One scheduled OTA upgrade.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OtaJob {
    /// Device IEEE address.
    pub device_ieee: u64,
    /// Manufacturer code from the ZCL Query Next Image Request.
    pub manufacturer_code: u16,
    /// Image type code.
    pub image_type: u16,
    /// Current firmware version reported by the device.
    pub current_file_version: u32,
    /// Status of the job (Pending → InFlight → Complete / Failed).
    pub status: OtaJobStatus,
}

/// Lifecycle status of an [`OtaJob`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OtaJobStatus {
    /// Waiting in the queue.
    Pending,
    /// Block transfer in progress.
    InFlight,
    /// Device sent Upgrade End Request with SUCCESS.
    Complete,
    /// Aborted (timeout, NO_IMAGE_AVAILABLE, signature failure).
    Failed,
}

/// A pluggable source of OTA images.
///
/// Callers (e.g. the Portal admin handler, or an in-tree firmware
/// catalogue) implement this trait to provide the next image for a
/// device. Phase 1 doesn't ship a concrete implementation — the
/// catalogue side lives in `cave-home-orchestration` once that crate
/// lands; for now this trait + [`NoImageProvider`] is the integration
/// seam.
#[async_trait]
pub trait OtaImageProvider: Send + Sync {
    /// Return the next image for `manufacturer_code` + `image_type` if
    /// the device's `current_version` is older.
    async fn next_image_for(
        &self,
        manufacturer_code: u16,
        image_type: u16,
        current_version: u32,
    ) -> Result<Option<OtaImageDescriptor>>;
}

/// Light-weight descriptor returned by the provider — references the
/// image by URL; the block-transfer transport in Phase 1b is responsible
/// for fetching and signing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OtaImageDescriptor {
    /// New image's file version.
    pub file_version: u32,
    /// Total image size in bytes.
    pub image_size: u32,
    /// Provider-specific URL (file:///, https://…).
    pub source: String,
}

/// Concrete provider that always responds "no image available". Useful
/// for tests and as a sane default when the catalogue isn't wired.
pub struct NoImageProvider;

#[async_trait]
impl OtaImageProvider for NoImageProvider {
    async fn next_image_for(
        &self,
        _manufacturer_code: u16,
        _image_type: u16,
        _current_version: u32,
    ) -> Result<Option<OtaImageDescriptor>> {
        Ok(None)
    }
}

/// OTA job queue + signal handler.
///
/// Cheap to clone (`Arc<Mutex<…>>` inside). Held by
/// [`crate::coordinator::Coordinator`].
#[derive(Clone)]
pub struct OtaQueue {
    inner: Arc<Mutex<HashMap<u64, OtaJob>>>,
    provider: Arc<dyn OtaImageProvider>,
}

impl OtaQueue {
    /// Construct a new queue backed by `provider`.
    #[must_use]
    pub fn new(provider: Arc<dyn OtaImageProvider>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            provider,
        }
    }

    /// Construct a no-image queue (uses [`NoImageProvider`]).
    #[must_use]
    pub fn no_image() -> Self {
        Self::new(Arc::new(NoImageProvider))
    }

    /// Enqueue / refresh the job for `device_ieee`.
    pub fn enqueue(&self, job: OtaJob) {
        self.inner.lock().insert(job.device_ieee, job);
    }

    /// Number of pending or in-flight jobs.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.inner
            .lock()
            .values()
            .filter(|j| {
                matches!(
                    j.status,
                    OtaJobStatus::Pending | OtaJobStatus::InFlight
                )
            })
            .count()
    }

    /// Look up a job by device.
    #[must_use]
    pub fn get(&self, device_ieee: u64) -> Option<OtaJob> {
        self.inner.lock().get(&device_ieee).cloned()
    }

    /// Mark a job as `status` (no-op if device unknown).
    pub fn set_status(&self, device_ieee: u64, status: OtaJobStatus) {
        if let Some(j) = self.inner.lock().get_mut(&device_ieee) {
            j.status = status;
        }
    }

    /// Drain completed jobs (returns them in insertion order).
    pub fn drain_completed(&self) -> Vec<OtaJob> {
        let mut guard = self.inner.lock();
        let keys: Vec<u64> = guard
            .iter()
            .filter(|(_, j)| j.status == OtaJobStatus::Complete)
            .map(|(k, _)| *k)
            .collect();
        keys.into_iter()
            .map(|k| guard.remove(&k).expect("just collected"))
            .collect()
    }

    /// Handle an incoming ZCL Query Next Image Request — consult the
    /// provider and queue a job if a newer image is available.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Pairing`] when the provider errors out
    /// (it shouldn't in practice; we still propagate so the caller can
    /// log it).
    pub async fn on_query_next_image(
        &self,
        device_ieee: u64,
        manufacturer_code: u16,
        image_type: u16,
        current_file_version: u32,
    ) -> Result<Option<OtaImageDescriptor>> {
        let desc = self
            .provider
            .next_image_for(manufacturer_code, image_type, current_file_version)
            .await?;
        if let Some(d) = &desc {
            if d.file_version > current_file_version {
                self.enqueue(OtaJob {
                    device_ieee,
                    manufacturer_code,
                    image_type,
                    current_file_version,
                    status: OtaJobStatus::Pending,
                });
            }
        }
        Ok(desc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysNewer;

    #[async_trait]
    impl OtaImageProvider for AlwaysNewer {
        async fn next_image_for(
            &self,
            _mc: u16,
            _it: u16,
            current: u32,
        ) -> Result<Option<OtaImageDescriptor>> {
            Ok(Some(OtaImageDescriptor {
                file_version: current + 1,
                image_size: 4096,
                source: "file:///tmp/test.ota".into(),
            }))
        }
    }

    struct AlwaysOlder;

    #[async_trait]
    impl OtaImageProvider for AlwaysOlder {
        async fn next_image_for(
            &self,
            _mc: u16,
            _it: u16,
            current: u32,
        ) -> Result<Option<OtaImageDescriptor>> {
            Ok(Some(OtaImageDescriptor {
                file_version: current.saturating_sub(1),
                image_size: 4096,
                source: "file:///tmp/old.ota".into(),
            }))
        }
    }

    #[tokio::test]
    async fn no_image_provider_returns_none() {
        let q = OtaQueue::no_image();
        let resp = q
            .on_query_next_image(0xaaaa, 0x100b, 0x0001, 1)
            .await
            .unwrap();
        assert!(resp.is_none());
        assert_eq!(q.in_flight(), 0);
    }

    #[tokio::test]
    async fn always_newer_provider_enqueues_job() {
        let q = OtaQueue::new(Arc::new(AlwaysNewer));
        let resp = q
            .on_query_next_image(0xaaaa, 0x100b, 0x0001, 5)
            .await
            .unwrap();
        assert!(resp.is_some());
        assert_eq!(q.in_flight(), 1);
        let job = q.get(0xaaaa).unwrap();
        assert_eq!(job.status, OtaJobStatus::Pending);
        assert_eq!(job.current_file_version, 5);
    }

    #[tokio::test]
    async fn always_older_provider_does_not_enqueue() {
        let q = OtaQueue::new(Arc::new(AlwaysOlder));
        let _ = q
            .on_query_next_image(0xaaaa, 0x100b, 0x0001, 5)
            .await
            .unwrap();
        assert_eq!(q.in_flight(), 0);
    }

    #[tokio::test]
    async fn set_status_transitions_through_lifecycle() {
        let q = OtaQueue::new(Arc::new(AlwaysNewer));
        q.on_query_next_image(0xaaaa, 0, 0, 1).await.unwrap();
        q.set_status(0xaaaa, OtaJobStatus::InFlight);
        assert_eq!(q.in_flight(), 1);
        q.set_status(0xaaaa, OtaJobStatus::Complete);
        assert_eq!(q.in_flight(), 0);
        let drained = q.drain_completed();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].device_ieee, 0xaaaa);
    }

    #[tokio::test]
    async fn failed_jobs_do_not_show_up_in_flight() {
        let q = OtaQueue::new(Arc::new(AlwaysNewer));
        q.on_query_next_image(0xaaaa, 0, 0, 1).await.unwrap();
        q.set_status(0xaaaa, OtaJobStatus::Failed);
        assert_eq!(q.in_flight(), 0);
    }
}
