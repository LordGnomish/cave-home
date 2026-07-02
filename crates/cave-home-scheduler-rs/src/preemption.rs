// SPDX-License-Identifier: Apache-2.0
//! Priority-based preemption — `DefaultPreemption` PostFilter plugin.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/defaultpreemption/default_preemption.go
//!
//! Phase 2 scope: find any node whose lower-priority pods, if all removed,
//! would let the incoming pod fit. Lower-priority-pod *minimisation*
//! (the upstream `selectVictimsOnNode` cost/regret optimisation) is
//! deferred — see `parity.manifest.toml`.

use crate::cache::NodeInfo;
use crate::framework::{
    CycleState, FilterFailureMap, PluginRegistry, PostFilterPlugin, Status,
};
use crate::types::Pod;

/// Upstream: `defaultpreemption.DefaultPreemption`.
#[derive(Default)]
pub struct DefaultPreemption;

impl DefaultPreemption {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Registry-driven preemption (upstream `DefaultPreemption.PostFilter` →
    /// `FindCandidates` → `SelectVictimsOnNode`).
    ///
    /// Honours the caller's actual [`PluginRegistry`]: it runs every `PreFilter`
    /// plugin once to seed [`CycleState`], then — as it removes victim pods from
    /// a candidate node — drives each plugin's
    /// [`PreFilterExtensions::remove_pod`](crate::framework::PreFilterExtensions::remove_pod)
    /// hook so plugins that cached pod-level state stay consistent, before
    /// re-running the registry's `Filter` plugins on the hypothetical node. This
    /// is the spec-faithful interaction between preemption and the
    /// `PreFilterExtensions` extension point. The [`PostFilterPlugin`] trait impl
    /// delegates straight here.
    #[must_use]
    pub fn post_filter_with_registry(
        &self,
        pod: &Pod,
        nodes: &[NodeInfo],
        registry: &PluginRegistry,
    ) -> (Option<String>, Status) {
        let candidate_priority = pod.spec.priority;
        // `best` tracks the lowest-cost candidate so far; cost = victim count.
        let mut best: Option<(String, usize)> = None;

        for info in nodes {
            // Eviction candidates: strictly-lower-priority pods, cheapest first.
            let mut victims: Vec<Pod> = info
                .pods()
                .iter()
                .filter(|p| p.spec.priority < candidate_priority)
                .cloned()
                .collect();
            victims.sort_by_key(|p| p.spec.priority);

            // Greedily remove victims until the pod fits, replaying RemovePod
            // into a fresh cycle state seeded by PreFilter each time so the
            // hypothetical reflects exactly the removed set.
            let mut chosen: Vec<Pod> = Vec::new();
            let mut fits = Self::fits_with_registry(pod, info, &chosen, registry);
            for v in victims {
                if fits {
                    break;
                }
                chosen.push(v);
                fits = Self::fits_with_registry(pod, info, &chosen, registry);
            }

            if chosen.is_empty() || !fits {
                continue;
            }
            let cost = chosen.len();
            let better = match &best {
                None => true,
                Some((_, prev)) => cost < *prev,
            };
            if better {
                best = Some((info.node().metadata.name.clone(), cost));
            }
        }

        match best {
            Some((name, _)) => {
                let reasons = vec![format!("nominated node {name} after preemption")];
                (
                    Some(name),
                    Status {
                        code: crate::framework::Code::Success,
                        plugin: self.name().into(),
                        reasons,
                    },
                )
            }
            None => (
                None,
                Status::unschedulable(self.name(), "preemption found no candidate node"),
            ),
        }
    }

    /// Hypothetical feasibility of `pod` on `info` once `victims` are removed,
    /// evaluated through the registry's `PreFilter` (+ `RemovePod` extensions)
    /// and `Filter` plugins. The cycle state is seeded once by `PreFilter`, then
    /// each victim removal is replayed via every plugin's `RemovePod` hook so
    /// cached pod-level state stays consistent before `Filter` runs on the
    /// mutated node.
    fn fits_with_registry(
        pod: &Pod,
        info: &NodeInfo,
        victims: &[Pod],
        registry: &PluginRegistry,
    ) -> bool {
        let mut state = CycleState::new();

        // PreFilter seeds pod-level cycle state. A non-success PreFilter means
        // the pod can never fit here regardless of eviction.
        for plugin in registry.pre_filters() {
            let (_, status) = plugin.pre_filter(&mut state, pod);
            if !status.is_success() {
                return false;
            }
        }

        // Hypothetically remove each victim, driving the RemovePod extension so
        // PreFilter plugins update their cached state for the mutated node.
        let mut hypo = info.clone();
        for victim in victims {
            for plugin in registry.pre_filters() {
                if let Some(ext) = plugin.pre_filter_extensions() {
                    let status = ext.remove_pod(&mut state, pod, victim, &hypo);
                    if !status.is_success() {
                        return false;
                    }
                }
            }
            hypo.remove_pod(victim);
        }

        // Re-run the registry's Filter plugins on the hypothetical node.
        registry
            .filters()
            .iter()
            .all(|f| f.filter(&mut state, pod, &hypo).is_success())
    }

}

impl PostFilterPlugin for DefaultPreemption {
    fn name(&self) -> &'static str {
        "DefaultPreemption"
    }

    fn post_filter(
        &self,
        _state: &mut CycleState,
        pod: &Pod,
        nodes: &[NodeInfo],
        _filter_failures: &FilterFailureMap,
        registry: &PluginRegistry,
    ) -> (Option<String>, Status) {
        // Delegate to the registry-driven path: it runs the profile's real
        // PreFilter (+ PreFilterExtensions) and Filter plugins while greedily
        // evicting the cheapest lower-priority victims until the pod fits.
        self.post_filter_with_registry(pod, nodes, registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Container, Node, Pod, Quantity, ResourceName};

    fn node(name: &str, cpu_cap: i64) -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = name.into();
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_cap));
        n.status
            .allocatable
            .insert(ResourceName::Memory, Quantity::bytes(8192));
        NodeInfo::new(n)
    }

    fn pod(name: &str, priority: i32, cpu_m: i64) -> Pod {
        let mut p = Pod::default();
        p.metadata.name = name.into();
        p.metadata.uid = name.into();
        p.spec.priority = priority;
        let mut c = Container::default();
        if cpu_m > 0 {
            c.resources
                .requests
                .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
        }
        p.spec.containers.push(c);
        p
    }

    #[test]
    fn preemption_picks_node_after_evicting_lower_priority() {
        let mut info = node("n1", 1000);
        let victim = pod("victim", 0, 800);
        info.add_pod(victim);
        let incoming = pod("important", 100, 500);
        let mut s = CycleState::new();
        let reg = crate::plugins::default_registry();
        let (nominee, status) = DefaultPreemption::new().post_filter(
            &mut s,
            &incoming,
            &[info],
            &FilterFailureMap::new(),
            &reg,
        );
        assert!(status.is_success(), "{:?}", status);
        assert_eq!(nominee.as_deref(), Some("n1"));
    }

    #[test]
    fn preemption_fails_when_no_lower_priority_victims() {
        let mut info = node("n1", 1000);
        let stayer = pod("stayer", 200, 800);
        info.add_pod(stayer);
        let incoming = pod("important", 100, 500);
        let mut s = CycleState::new();
        let reg = crate::plugins::default_registry();
        let (nominee, status) = DefaultPreemption::new().post_filter(
            &mut s,
            &incoming,
            &[info],
            &FilterFailureMap::new(),
            &reg,
        );
        assert!(nominee.is_none());
        assert!(!status.is_success());
    }

    // ---------- PreFilterExtensions (AddPod/RemovePod) during preemption ----

    use crate::framework::{
        FilterPlugin, PluginRegistry, PreFilterExtensions, PreFilterPlugin, PreFilterResult, Status,
    };
    use std::sync::Arc;

    /// A PreFilter plugin that caches, in cycle state, how many *blocking* pods
    /// sit on each node (here: pods named "victim"). Its Filter rejects any node
    /// whose cached blocking-count is non-zero. The count is only ever brought
    /// down by the RemovePod extension — so the node becomes feasible *only* if
    /// preemption drives RemovePod for the victim. A naive re-filter that
    /// rebuilt state from scratch would never consult this hook and would keep
    /// the node infeasible, so this isolates the extension path.
    #[derive(Default)]
    struct BlockingCount;

    const BLOCK_KEY: &str = "BlockingCount";

    impl PreFilterPlugin for BlockingCount {
        fn name(&self) -> &'static str {
            "BlockingCount"
        }
        fn pre_filter(
            &self,
            state: &mut CycleState,
            _: &Pod,
        ) -> (Option<PreFilterResult>, Status) {
            // Seed: assume one blocking pod is present (the victim).
            state.write(BLOCK_KEY, 1_i64);
            (None, Status::success())
        }
        fn pre_filter_extensions(&self) -> Option<&dyn PreFilterExtensions> {
            Some(self)
        }
    }

    impl PreFilterExtensions for BlockingCount {
        fn add_pod(&self, state: &mut CycleState, _: &Pod, _: &Pod, _: &NodeInfo) -> Status {
            let n = state.read::<i64>(BLOCK_KEY).copied().unwrap_or(0);
            state.write(BLOCK_KEY, n + 1);
            Status::success()
        }
        fn remove_pod(
            &self,
            state: &mut CycleState,
            _: &Pod,
            pod_to_remove: &Pod,
            _: &NodeInfo,
        ) -> Status {
            if pod_to_remove.metadata.name == "victim" {
                let n = state.read::<i64>(BLOCK_KEY).copied().unwrap_or(0);
                state.write(BLOCK_KEY, n - 1);
            }
            Status::success()
        }
    }

    impl FilterPlugin for BlockingCount {
        fn name(&self) -> &'static str {
            "BlockingCount"
        }
        fn filter(&self, state: &mut CycleState, _: &Pod, _: &NodeInfo) -> Status {
            let n = state.read::<i64>(BLOCK_KEY).copied().unwrap_or(0);
            if n > 0 {
                Status::unschedulable("BlockingCount", "blocking pods present")
            } else {
                Status::success()
            }
        }
    }

    #[test]
    fn preemption_drives_remove_pod_extension_to_make_node_feasible() {
        // n1 has plenty of CPU, but the BlockingCount PreFilter makes it
        // infeasible until the victim is removed *via the RemovePod extension*.
        let mut info = node("n1", 10_000);
        let victim = pod("victim", 0, 100);
        info.add_pod(victim);
        let incoming = pod("important", 100, 100);

        let reg = PluginRegistry::builder()
            .with_pre_filter(Arc::new(BlockingCount))
            .with_filter(Arc::new(BlockingCount))
            .build();

        let (nominee, status) =
            DefaultPreemption::new().post_filter_with_registry(&incoming, &[info], &reg);
        assert!(status.is_success(), "{status:?}");
        assert_eq!(nominee.as_deref(), Some("n1"));
    }

    #[test]
    fn preemption_via_registry_fails_when_no_victim_unblocks_node() {
        // Same gate, but the only pod is a *higher*-priority stayer that is not
        // an eviction candidate, so RemovePod never runs and the node stays
        // infeasible.
        let mut info = node("n1", 10_000);
        let stayer = pod("stayer", 500, 100); // priority 500 > incoming 100
        info.add_pod(stayer);
        let incoming = pod("important", 100, 100);

        let reg = PluginRegistry::builder()
            .with_pre_filter(Arc::new(BlockingCount))
            .with_filter(Arc::new(BlockingCount))
            .build();

        let (nominee, status) =
            DefaultPreemption::new().post_filter_with_registry(&incoming, &[info], &reg);
        assert!(nominee.is_none());
        assert!(!status.is_success());
    }

    #[test]
    fn preemption_skips_nodes_where_eviction_still_does_not_fit() {
        // n1 caps at 100m CPU; a low-priority victim holds 50m. Even after
        // evicting it the 9000m incoming pod cannot fit, so no nomination.
        let mut info = node("n1", 100);
        info.add_pod(pod("victim", 0, 50));
        let incoming = pod("important", 100, 9000);
        let mut s = CycleState::new();
        let reg = crate::plugins::default_registry();
        let (nominee, _) = DefaultPreemption::new().post_filter(
            &mut s,
            &incoming,
            &[info],
            &FilterFailureMap::new(),
            &reg,
        );
        assert!(nominee.is_none());
    }
}
