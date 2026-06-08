// SPDX-License-Identifier: Apache-2.0
//! Plugin registry — holds the ordered Filter and Score plugin sets.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/runtime/registry.go
//!         pkg/scheduler/framework/runtime/framework.go

use std::sync::Arc;

use super::{
    BindPlugin, FilterPlugin, PermitPlugin, PostBindPlugin, PostFilterPlugin, PreBindPlugin,
    PreEnqueuePlugin, PreFilterPlugin, PreScorePlugin, QueueSortPlugin, ReservePlugin, ScorePlugin,
};

/// Upstream: `pkg/scheduler/framework/runtime/framework.go::frameworkImpl`.
///
/// We diverge from upstream's reflective `runtime.Registry` (a name->factory
/// map) because Rust generics + traits give us static dispatch without the
/// reflection ceremony. The behavioural contract — ordered execution of
/// each extension point — is preserved 1:1.
#[derive(Clone, Default)]
pub struct PluginRegistry {
    pre_enqueues: Vec<Arc<dyn PreEnqueuePlugin>>,
    queue_sort: Option<Arc<dyn QueueSortPlugin>>,
    pre_filters: Vec<Arc<dyn PreFilterPlugin>>,
    filters: Vec<Arc<dyn FilterPlugin>>,
    pre_scores: Vec<Arc<dyn PreScorePlugin>>,
    scores: Vec<Arc<dyn ScorePlugin>>,
    post_filters: Vec<Arc<dyn PostFilterPlugin>>,
    reserves: Vec<Arc<dyn ReservePlugin>>,
    permits: Vec<Arc<dyn PermitPlugin>>,
    pre_binds: Vec<Arc<dyn PreBindPlugin>>,
    binds: Vec<Arc<dyn BindPlugin>>,
    post_binds: Vec<Arc<dyn PostBindPlugin>>,
}

impl PluginRegistry {
    #[must_use]
    pub fn builder() -> RegistryBuilder {
        RegistryBuilder::default()
    }

    #[must_use]
    pub fn pre_enqueues(&self) -> &[Arc<dyn PreEnqueuePlugin>] {
        &self.pre_enqueues
    }

    #[must_use]
    pub fn queue_sort(&self) -> Option<&Arc<dyn QueueSortPlugin>> {
        self.queue_sort.as_ref()
    }

    #[must_use]
    pub fn reserves(&self) -> &[Arc<dyn ReservePlugin>] {
        &self.reserves
    }

    #[must_use]
    pub fn binds(&self) -> &[Arc<dyn BindPlugin>] {
        &self.binds
    }

    #[must_use]
    pub fn post_binds(&self) -> &[Arc<dyn PostBindPlugin>] {
        &self.post_binds
    }

    #[must_use]
    pub fn permits(&self) -> &[Arc<dyn PermitPlugin>] {
        &self.permits
    }

    #[must_use]
    pub fn pre_binds(&self) -> &[Arc<dyn PreBindPlugin>] {
        &self.pre_binds
    }

    #[must_use]
    pub fn pre_filters(&self) -> &[Arc<dyn PreFilterPlugin>] {
        &self.pre_filters
    }

    #[must_use]
    pub fn filters(&self) -> &[Arc<dyn FilterPlugin>] {
        &self.filters
    }

    #[must_use]
    pub fn pre_scores(&self) -> &[Arc<dyn PreScorePlugin>] {
        &self.pre_scores
    }

    #[must_use]
    pub fn scores(&self) -> &[Arc<dyn ScorePlugin>] {
        &self.scores
    }

    #[must_use]
    pub fn post_filters(&self) -> &[Arc<dyn PostFilterPlugin>] {
        &self.post_filters
    }
}

/// Builder for [`PluginRegistry`].
#[derive(Default)]
pub struct RegistryBuilder {
    pre_enqueues: Vec<Arc<dyn PreEnqueuePlugin>>,
    queue_sort: Option<Arc<dyn QueueSortPlugin>>,
    pre_filters: Vec<Arc<dyn PreFilterPlugin>>,
    filters: Vec<Arc<dyn FilterPlugin>>,
    pre_scores: Vec<Arc<dyn PreScorePlugin>>,
    scores: Vec<Arc<dyn ScorePlugin>>,
    post_filters: Vec<Arc<dyn PostFilterPlugin>>,
    reserves: Vec<Arc<dyn ReservePlugin>>,
    permits: Vec<Arc<dyn PermitPlugin>>,
    pre_binds: Vec<Arc<dyn PreBindPlugin>>,
    binds: Vec<Arc<dyn BindPlugin>>,
    post_binds: Vec<Arc<dyn PostBindPlugin>>,
}

impl RegistryBuilder {
    #[must_use]
    pub fn with_pre_enqueue(mut self, p: Arc<dyn PreEnqueuePlugin>) -> Self {
        self.pre_enqueues.push(p);
        self
    }

    /// Set the (single) QueueSort plugin. Upstream allows exactly one enabled
    /// QueueSort plugin per profile; a second call replaces the first.
    #[must_use]
    pub fn with_queue_sort(mut self, p: Arc<dyn QueueSortPlugin>) -> Self {
        self.queue_sort = Some(p);
        self
    }

    #[must_use]
    pub fn with_pre_filter(mut self, p: Arc<dyn PreFilterPlugin>) -> Self {
        self.pre_filters.push(p);
        self
    }

    #[must_use]
    pub fn with_bind(mut self, p: Arc<dyn BindPlugin>) -> Self {
        self.binds.push(p);
        self
    }

    #[must_use]
    pub fn with_post_bind(mut self, p: Arc<dyn PostBindPlugin>) -> Self {
        self.post_binds.push(p);
        self
    }

    #[must_use]
    pub fn with_reserve(mut self, p: Arc<dyn ReservePlugin>) -> Self {
        self.reserves.push(p);
        self
    }

    #[must_use]
    pub fn with_permit(mut self, p: Arc<dyn PermitPlugin>) -> Self {
        self.permits.push(p);
        self
    }

    #[must_use]
    pub fn with_pre_bind(mut self, p: Arc<dyn PreBindPlugin>) -> Self {
        self.pre_binds.push(p);
        self
    }

    #[must_use]
    pub fn with_filter(mut self, p: Arc<dyn FilterPlugin>) -> Self {
        self.filters.push(p);
        self
    }

    #[must_use]
    pub fn with_pre_score(mut self, p: Arc<dyn PreScorePlugin>) -> Self {
        self.pre_scores.push(p);
        self
    }

    #[must_use]
    pub fn with_score(mut self, p: Arc<dyn ScorePlugin>) -> Self {
        self.scores.push(p);
        self
    }

    #[must_use]
    pub fn with_post_filter(mut self, p: Arc<dyn PostFilterPlugin>) -> Self {
        self.post_filters.push(p);
        self
    }

    #[must_use]
    pub fn build(self) -> PluginRegistry {
        PluginRegistry {
            pre_enqueues: self.pre_enqueues,
            queue_sort: self.queue_sort,
            pre_filters: self.pre_filters,
            filters: self.filters,
            pre_scores: self.pre_scores,
            scores: self.scores,
            post_filters: self.post_filters,
            reserves: self.reserves,
            permits: self.permits,
            pre_binds: self.pre_binds,
            binds: self.binds,
            post_binds: self.post_binds,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::cache::NodeInfo;
    use crate::framework::{CycleState, Status};
    use crate::types::Pod;

    struct PassFilter;
    impl FilterPlugin for PassFilter {
        fn name(&self) -> &'static str {
            "PassFilter"
        }
        fn filter(&self, _: &mut CycleState, _: &Pod, _: &NodeInfo) -> Status {
            Status::success()
        }
    }

    struct ZeroScore;
    impl ScorePlugin for ZeroScore {
        fn name(&self) -> &'static str {
            "ZeroScore"
        }
        fn score(&self, _: &mut CycleState, _: &Pod, _: &NodeInfo) -> (i64, Status) {
            (0, Status::success())
        }
    }

    #[test]
    fn registry_records_filter_and_score_plugins_in_order() {
        let reg = PluginRegistry::builder()
            .with_filter(Arc::new(PassFilter))
            .with_score(Arc::new(ZeroScore))
            .build();
        assert_eq!(reg.filters().len(), 1);
        assert_eq!(reg.scores().len(), 1);
        assert_eq!(reg.filters()[0].name(), "PassFilter");
        assert_eq!(reg.scores()[0].name(), "ZeroScore");
    }

    struct NoopPreFilter;
    impl super::super::PreFilterPlugin for NoopPreFilter {
        fn name(&self) -> &'static str {
            "NoopPreFilter"
        }
        fn pre_filter(
            &self,
            _: &mut CycleState,
            _: &Pod,
        ) -> (Option<super::super::PreFilterResult>, Status) {
            (None, Status::success())
        }
    }

    struct NoopPreScore;
    impl super::super::PreScorePlugin for NoopPreScore {
        fn name(&self) -> &'static str {
            "NoopPreScore"
        }
        fn pre_score(&self, _: &mut CycleState, _: &Pod, _: &[NodeInfo]) -> Status {
            Status::success()
        }
    }

    #[test]
    fn registry_records_pre_filter_and_pre_score_plugins_in_order() {
        let reg = PluginRegistry::builder()
            .with_pre_filter(Arc::new(NoopPreFilter))
            .with_pre_score(Arc::new(NoopPreScore))
            .build();
        assert_eq!(reg.pre_filters().len(), 1);
        assert_eq!(reg.pre_scores().len(), 1);
        assert_eq!(reg.pre_filters()[0].name(), "NoopPreFilter");
        assert_eq!(reg.pre_scores()[0].name(), "NoopPreScore");
    }

    struct AdmitPreEnqueue;
    impl super::super::PreEnqueuePlugin for AdmitPreEnqueue {
        fn name(&self) -> &'static str {
            "AdmitPreEnqueue"
        }
        fn pre_enqueue(&self, _: &Pod) -> Status {
            Status::success()
        }
    }

    struct PrioritySort;
    impl super::super::QueueSortPlugin for PrioritySort {
        fn name(&self) -> &'static str {
            "PrioritySort"
        }
        fn less(&self, a: &Pod, b: &Pod) -> bool {
            a.spec.priority > b.spec.priority
        }
    }

    #[test]
    fn registry_records_pre_enqueue_and_queue_sort_plugins() {
        let reg = PluginRegistry::builder()
            .with_pre_enqueue(Arc::new(AdmitPreEnqueue))
            .with_queue_sort(Arc::new(PrioritySort))
            .build();
        assert_eq!(reg.pre_enqueues().len(), 1);
        assert_eq!(reg.pre_enqueues()[0].name(), "AdmitPreEnqueue");
        assert!(reg.queue_sort().is_some());
        assert_eq!(reg.queue_sort().unwrap().name(), "PrioritySort");
    }
}
