//! Observability metrics for the provisioner (Track 4).
//!
//! These are the operational signals an operator watches: how many provisioned
//! `PersistentVolume`s exist in each phase, how many provisions / deletions have
//! run (and how many provisions failed), the reconcile-error counter (the
//! numerator of the error *rate*), and a provisioning-latency summary. They are
//! computed purely from observed state and rendered as Prometheus text
//! exposition, matching the house ServiceLB-controller convention. No clock and
//! no metrics registry live here: the caller observes phases, feeds counters and
//! records latencies; the scrape/registry wiring is ADR-004 phase-1b.

/// A `PersistentVolume` lifecycle phase (Kubernetes `PersistentVolumePhase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PvPhase {
    /// Not yet available for binding.
    Pending,
    /// Available — a free resource not yet bound to a claim.
    Available,
    /// Bound — bound to a claim.
    Bound,
    /// Released — the claim was deleted but the resource is not yet reclaimed.
    Released,
    /// Failed — automatic reclamation failed.
    Failed,
}

impl PvPhase {
    /// Every phase, in a stable order (the gauge label set).
    pub const ALL: [Self; 5] = [
        Self::Pending,
        Self::Available,
        Self::Bound,
        Self::Released,
        Self::Failed,
    ];

    /// The Kubernetes API phase name (also the gauge label value).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Available => "Available",
            Self::Bound => "Bound",
            Self::Released => "Released",
            Self::Failed => "Failed",
        }
    }
}

/// The provisioner's metric snapshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalPathMetrics {
    /// Total provisioned PVs (all phases).
    pub pvs_total: u64,
    pvs_pending: u64,
    pvs_available: u64,
    pvs_bound: u64,
    pvs_released: u64,
    pvs_failed: u64,
    /// Total provision attempts (counter).
    pub provisions_total: u64,
    /// Provision attempts that failed (counter).
    pub provision_failures_total: u64,
    /// Total delete/reclaim runs (counter).
    pub deletions_total: u64,
    /// Reconcile errors (counter; the error-rate numerator).
    pub reconcile_errors_total: u64,
    /// Sum of observed provisioning latencies, in seconds.
    pub provision_latency_seconds_sum: f64,
    /// Count of observed provisioning latencies.
    pub provision_latency_seconds_count: u64,
}

impl LocalPathMetrics {
    /// Snapshot the PV-phase distribution from the observed phases.
    #[must_use]
    pub fn observe(phases: &[PvPhase]) -> Self {
        let mut m = Self {
            pvs_total: phases.len() as u64,
            ..Self::default()
        };
        for phase in phases {
            match phase {
                PvPhase::Pending => m.pvs_pending += 1,
                PvPhase::Available => m.pvs_available += 1,
                PvPhase::Bound => m.pvs_bound += 1,
                PvPhase::Released => m.pvs_released += 1,
                PvPhase::Failed => m.pvs_failed += 1,
            }
        }
        m
    }

    /// The PV count in a given phase.
    #[must_use]
    pub const fn pvs_by_phase(&self, phase: PvPhase) -> u64 {
        match phase {
            PvPhase::Pending => self.pvs_pending,
            PvPhase::Available => self.pvs_available,
            PvPhase::Bound => self.pvs_bound,
            PvPhase::Released => self.pvs_released,
            PvPhase::Failed => self.pvs_failed,
        }
    }

    /// Set the provision counters (total attempts and failures).
    #[must_use]
    pub const fn with_provisions(mut self, total: u64, failures: u64) -> Self {
        self.provisions_total = total;
        self.provision_failures_total = failures;
        self
    }

    /// Set the deletion counter.
    #[must_use]
    pub const fn with_deletions(mut self, total: u64) -> Self {
        self.deletions_total = total;
        self
    }

    /// Set the reconcile-error counter.
    #[must_use]
    pub const fn with_reconcile_errors(mut self, total: u64) -> Self {
        self.reconcile_errors_total = total;
        self
    }

    /// Record one provisioning latency observation (seconds), accumulating the
    /// summary sum and count.
    #[must_use]
    pub fn record_latency_seconds(mut self, seconds: f64) -> Self {
        self.provision_latency_seconds_sum += seconds;
        self.provision_latency_seconds_count += 1;
        self
    }

    /// Render the metrics as Prometheus text exposition.
    #[must_use]
    pub fn to_prometheus(&self) -> String {
        use core::fmt::Write as _;
        let mut out = String::new();

        out.push_str("# HELP localpath_pvs Provisioned PersistentVolumes by phase.\n");
        out.push_str("# TYPE localpath_pvs gauge\n");
        for phase in PvPhase::ALL {
            let _ = writeln!(
                out,
                "localpath_pvs{{phase=\"{}\"}} {}",
                phase.as_str(),
                self.pvs_by_phase(phase)
            );
        }

        for (name, kind, value) in [
            ("localpath_pvs_total", "gauge", self.pvs_total),
            ("localpath_provisions_total", "counter", self.provisions_total),
            (
                "localpath_provision_failures_total",
                "counter",
                self.provision_failures_total,
            ),
            ("localpath_deletions_total", "counter", self.deletions_total),
            (
                "localpath_reconcile_errors_total",
                "counter",
                self.reconcile_errors_total,
            ),
        ] {
            let _ = writeln!(out, "# TYPE {name} {kind}\n{name} {value}");
        }

        out.push_str(
            "# HELP localpath_provision_latency_seconds Provisioning latency summary.\n",
        );
        out.push_str("# TYPE localpath_provision_latency_seconds summary\n");
        let _ = writeln!(
            out,
            "localpath_provision_latency_seconds_sum {}",
            self.provision_latency_seconds_sum
        );
        let _ = writeln!(
            out,
            "localpath_provision_latency_seconds_count {}",
            self.provision_latency_seconds_count
        );

        out
    }
}
