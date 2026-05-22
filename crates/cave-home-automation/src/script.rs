// SPDX-License-Identifier: Apache-2.0
//! Script runner — port of `homeassistant/helpers/script.py`.
//!
//! A script is a sequence of [`Action`]s executed in order; each
//! action variant maps 1:1 to one of HA's `_async_step_*` methods.
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/helpers/script.py

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::sleep;

use crate::automation::conditions::Condition;
use crate::automation::triggers::Trigger;
use crate::context::Context;
use crate::error::{HassError, HassResult};
use crate::event_bus::{EVENT_STATE_CHANGED, Event, EventBus, ListenerFn};
use crate::service::ServiceRegistry;
use crate::state::StateMachine;

/// Sequence of [`Action`]s.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/helpers/script.py::Script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    pub name: String,
    pub sequence: Vec<Action>,
}

/// One step inside a [`Script`] sequence.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/helpers/script.py
///   (`_async_step_*` methods)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Call a service.
    ///
    /// # Upstream: homeassistant/helpers/script.py::_async_step_call_service
    Service {
        domain: String,
        service: String,
        #[serde(default)]
        data: Value,
    },

    /// Activate a scene by id.
    ///
    /// # Upstream: homeassistant/helpers/script.py::_async_step_scene
    Scene { scene_id: String },

    /// Sleep for `milliseconds` ms.
    ///
    /// # Upstream: homeassistant/helpers/script.py::_async_step_delay
    Delay { milliseconds: u64 },

    /// Wait until a [`Trigger`] fires (or `timeout_ms` elapses).
    ///
    /// # Upstream: homeassistant/helpers/script.py::_async_step_wait_for_trigger
    WaitForTrigger {
        trigger: Trigger,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },

    /// Repeat the body `count` times.
    ///
    /// # Upstream: homeassistant/helpers/script.py::_async_step_repeat
    Repeat { count: u32, sequence: Vec<Action> },

    /// First branch whose `conditions` pass runs its sequence; if none
    /// matches and `default` is set, that runs.
    ///
    /// # Upstream: homeassistant/helpers/script.py::_async_step_choose
    Choose {
        choose: Vec<ChooseBranch>,
        #[serde(default)]
        default: Vec<Action>,
    },

    /// Conditional execution.
    ///
    /// # Upstream: homeassistant/helpers/script.py::_async_step_if
    If {
        #[serde(rename = "if")]
        condition: Condition,
        then: Vec<Action>,
        #[serde(default)]
        otherwise: Vec<Action>,
    },
}

/// One branch of a `choose` action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChooseBranch {
    pub conditions: Vec<Condition>,
    pub sequence: Vec<Action>,
}

/// Resources passed through a script execution.
#[derive(Clone)]
pub struct ScriptContext {
    pub sm: Arc<StateMachine>,
    pub services: Arc<ServiceRegistry>,
    pub bus: Arc<dyn EventBus>,
    pub scenes: Arc<crate::scene::SceneRegistry>,
}

impl ScriptContext {
    #[must_use]
    pub fn new(
        sm: Arc<StateMachine>,
        services: Arc<ServiceRegistry>,
        bus: Arc<dyn EventBus>,
        scenes: Arc<crate::scene::SceneRegistry>,
    ) -> Self {
        Self {
            sm,
            services,
            bus,
            scenes,
        }
    }
}

impl Script {
    /// New named script.
    #[must_use]
    pub fn new(name: impl Into<String>, sequence: Vec<Action>) -> Self {
        Self {
            name: name.into(),
            sequence,
        }
    }

    /// Run the script to completion.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/helpers/script.py::Script.async_run
    pub async fn run(
        &self,
        ctx: &ScriptContext,
        context: Option<Context>,
    ) -> HassResult<()> {
        let cx = context.unwrap_or_default();
        run_sequence(&self.sequence, ctx, &cx).await
    }
}

#[allow(clippy::too_many_lines)]
async fn run_sequence(
    sequence: &[Action],
    rctx: &ScriptContext,
    context: &Context,
) -> HassResult<()> {
    for action in sequence {
        execute(action, rctx, context).await?;
    }
    Ok(())
}

fn execute<'a>(
    action: &'a Action,
    rctx: &'a ScriptContext,
    context: &'a Context,
) -> futures::future::BoxFuture<'a, HassResult<()>> {
    Box::pin(async move {
        match action {
            Action::Service { domain, service, data } => {
                rctx.services
                    .call(
                        domain,
                        service,
                        Some(data.clone()),
                        Some(context.clone()),
                        false,
                    )
                    .await?;
                Ok(())
            }
            Action::Scene { scene_id } => {
                let scene = rctx
                    .scenes
                    .get(scene_id)
                    .ok_or_else(|| HassError::Other(format!("unknown scene: {scene_id}")))?;
                scene.activate(&rctx.sm, Some(context.clone()))
            }
            Action::Delay { milliseconds } => {
                sleep(Duration::from_millis(*milliseconds)).await;
                Ok(())
            }
            Action::WaitForTrigger { trigger, timeout_ms } => {
                wait_for_trigger(trigger, *timeout_ms, rctx).await
            }
            Action::Repeat { count, sequence } => {
                for _ in 0..*count {
                    run_sequence(sequence, rctx, context).await?;
                }
                Ok(())
            }
            Action::Choose { choose, default } => {
                for branch in choose {
                    let all_pass = branch
                        .conditions
                        .iter()
                        .try_fold(true, |acc, c| -> HassResult<bool> {
                            Ok(acc && c.evaluate(&rctx.sm)?)
                        })?;
                    if all_pass {
                        return run_sequence(&branch.sequence, rctx, context).await;
                    }
                }
                run_sequence(default, rctx, context).await
            }
            Action::If { condition, then, otherwise } => {
                if condition.evaluate(&rctx.sm)? {
                    run_sequence(then, rctx, context).await
                } else {
                    run_sequence(otherwise, rctx, context).await
                }
            }
        }
    })
}

async fn wait_for_trigger(
    trigger: &Trigger,
    timeout_ms: Option<u64>,
    rctx: &ScriptContext,
) -> HassResult<()> {
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let trigger_clone = trigger.clone();
    let sm_for_listener = rctx.sm.clone();
    let event_type = trigger
        .subscribed_event_type()
        .unwrap_or(EVENT_STATE_CHANGED);
    let cb: ListenerFn = {
        let tx_for_cb = tx.clone();
        Arc::new(move |event: &Event| {
            if trigger_clone
                .matches(event, Some(&sm_for_listener))
                .unwrap_or(false)
            {
                if let Some(send) = tx_for_cb.lock().take() {
                    let _ = send.send(());
                }
            }
        })
    };
    let handle = rctx.bus.async_listen_dyn(event_type, cb);

    let result = if let Some(t) = timeout_ms {
        tokio::time::timeout(Duration::from_millis(t), rx).await
    } else {
        Ok(rx.await)
    };

    drop(handle);
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_recv_err)) => Err(HassError::Other("wait_for_trigger cancelled".into())),
        Err(_elapsed) => Err(HassError::Other("wait_for_trigger timed out".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::InMemoryEventBus;
    use crate::service::{service_handler, SupportsResponse};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_ctx() -> ScriptContext {
        let bus: Arc<dyn EventBus> = Arc::new(InMemoryEventBus::new());
        let sm = Arc::new(StateMachine::new(bus.clone()));
        let services = Arc::new(ServiceRegistry::new(bus.clone()));
        let scenes = Arc::new(crate::scene::SceneRegistry::new());
        ScriptContext::new(sm, services, bus, scenes)
    }

    /// Upstream-test: `tests/helpers/test_script.py::test_calling_service_basic`
    #[tokio::test]
    async fn script_runs_service_action() {
        let ctx = make_ctx();
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let handler = service_handler(move |_call| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        });
        ctx.services.register(
            "light",
            "turn_on",
            handler,
            None,
            SupportsResponse::None,
        );
        let script = Script::new(
            "Bedtime",
            vec![Action::Service {
                domain: "light".into(),
                service: "turn_on".into(),
                data: Value::Null,
            }],
        );
        script.run(&ctx, None).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    /// Upstream-test: `tests/helpers/test_script.py::test_delay_basic`
    #[tokio::test(start_paused = true)]
    async fn script_delay_waits() {
        let ctx = make_ctx();
        let script = Script::new(
            "Wait",
            vec![Action::Delay { milliseconds: 5_000 }],
        );
        let start = tokio::time::Instant::now();
        let fut = script.run(&ctx, None);
        tokio::pin!(fut);
        let advance = tokio::time::advance(Duration::from_secs(5));
        tokio::join!(advance, async {
            fut.as_mut().await.unwrap();
        });
        assert!(start.elapsed() >= Duration::from_secs(5));
    }

    /// Upstream-test: `tests/helpers/test_script.py::test_choose`
    #[tokio::test]
    async fn script_choose_picks_branch() {
        let ctx = make_ctx();
        ctx.sm
            .async_set("light.kitchen", "on", BTreeMap::new(), false, None)
            .unwrap();
        let flag = Arc::new(AtomicUsize::new(0));
        let f = flag.clone();
        let handler = service_handler(move |_call| {
            let f = f.clone();
            async move {
                f.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        });
        ctx.services
            .register("test", "mark", handler, None, SupportsResponse::None);
        let script = Script::new(
            "Choose",
            vec![Action::Choose {
                choose: vec![ChooseBranch {
                    conditions: vec![Condition::State {
                        entity_id: "light.kitchen".into(),
                        state: crate::automation::conditions::ConditionStateValue::Single(
                            "on".into(),
                        ),
                    }],
                    sequence: vec![Action::Service {
                        domain: "test".into(),
                        service: "mark".into(),
                        data: Value::Null,
                    }],
                }],
                default: vec![],
            }],
        );
        script.run(&ctx, None).await.unwrap();
        assert_eq!(flag.load(Ordering::SeqCst), 1);
    }

    /// Upstream-test: `tests/helpers/test_script.py::test_repeat_count`
    #[tokio::test]
    async fn script_repeat_iterates() {
        let ctx = make_ctx();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let handler = service_handler(move |_call| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        });
        ctx.services
            .register("test", "tick", handler, None, SupportsResponse::None);
        let script = Script::new(
            "Repeat",
            vec![Action::Repeat {
                count: 4,
                sequence: vec![Action::Service {
                    domain: "test".into(),
                    service: "tick".into(),
                    data: Value::Null,
                }],
            }],
        );
        script.run(&ctx, None).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 4);
    }

    /// Upstream-test: `tests/helpers/test_script.py::test_if_action`
    #[tokio::test]
    async fn script_if_branches() {
        let ctx = make_ctx();
        ctx.sm
            .async_set("light.kitchen", "off", BTreeMap::new(), false, None)
            .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let handler = service_handler(move |_call| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        });
        ctx.services
            .register("test", "other", handler, None, SupportsResponse::None);
        let script = Script::new(
            "If",
            vec![Action::If {
                condition: Condition::State {
                    entity_id: "light.kitchen".into(),
                    state: crate::automation::conditions::ConditionStateValue::Single(
                        "on".into(),
                    ),
                },
                then: vec![],
                otherwise: vec![Action::Service {
                    domain: "test".into(),
                    service: "other".into(),
                    data: Value::Null,
                }],
            }],
        );
        script.run(&ctx, None).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
