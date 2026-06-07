// SPDX-License-Identifier: Apache-2.0
//! Plugin registry — holds the ordered Filter and Score plugin sets.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/runtime/registry.go
//!         pkg/scheduler/framework/runtime/framework.go

use std::sync::Arc;

use super::{FilterPlugin, PostFilterPlugin, PreFilterPlugin, PreScorePlugin, ScorePlugin};

/// Upstream: `pkg/scheduler/framework/runtime/framework.go::frameworkImpl`.
///
/// We diverge from upstream's reflective `runtime.Registry` (a name->factory
/// map) because Rust generics + traits give us static dispatch without the
/// reflection ceremony. The behavioural contract — ordered execution of
/// each extension point — is preserved 1:1.
#[derive(Clone, Default)]
pub struct PluginRegistry {
    pre_filters: Vec<Arc<dyn PreFilterPlugin>>,
    filters: Vec<Arc<dyn FilterPlugin>>,
    pre_scores: Vec<Arc<dyn PreScorePlugin>>,
    scores: Vec<Arc<dyn ScorePlugin>>,
    post_filters: Vec<Arc<dyn PostFilterPlugin>>,
}

impl PluginRegistry {
    #[must_use]
    pub fn builder() -> RegistryBuilder {
        RegistryBuilder::default()
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
    pre_filters: Vec<Arc<dyn PreFilterPlugin>>,
    filters: Vec<Arc<dyn FilterPlugin>>,
    pre_scores: Vec<Arc<dyn PreScorePlugin>>,
    scores: Vec<Arc<dyn ScorePlugin>>,
    post_filters: Vec<Arc<dyn PostFilterPlugin>>,
}

impl RegistryBuilder {
    #[must_use]
    pub fn with_pre_filter(mut self, p: Arc<dyn PreFilterPlugin>) -> Self {
        self.pre_filters.push(p);
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
            pre_filters: self.pre_filters,
            filters: self.filters,
            pre_scores: self.pre_scores,
            scores: self.scores,
            post_filters: self.post_filters,
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
}
