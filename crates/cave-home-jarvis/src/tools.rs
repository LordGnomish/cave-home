// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The tool layer: the bridge from a decided intent (whether matched by
//! [`cave_home_voice`] or chosen by the LLM) to a cave-home **service call**.
//!
//! A [`ToolRegistry`] advertises the home's capabilities to the model as
//! JSON-Schema function specs and validates the calls that come back. A
//! [`ToolExecutor`] actually performs them against cave-home's services; the
//! real wiring (MQTT / the device crates) is the integration seam, and the
//! suite drives [`MockToolExecutor`].

use async_trait::async_trait;
use serde_json::{json, Value};

use cave_home_voice::route::{IntentAction, QueryKind};

use crate::error::{JarvisError, Result};
use crate::llm::{ToolCall, ToolSpec};

/// The outcome of executing one tool against the home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// Whether the service accepted the command.
    pub ok: bool,
    /// A short result to speak back and to feed to the model.
    pub message: String,
}

impl ToolResult {
    /// A success carrying a message.
    #[must_use]
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }

    /// A failure carrying a reason.
    #[must_use]
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

/// Translate a [`cave_home_voice`] intent into the matching tool call, so the
/// NLU fast-path and the LLM path converge on the *same* service surface.
#[must_use]
pub fn intent_to_tool_call(action: &IntentAction) -> ToolCall {
    match action {
        IntentAction::SetLight { target, on } => {
            ToolCall::new("set_light", json!({"target": target, "on": on}))
        }
        IntentAction::SetBrightness { target, percent } => {
            ToolCall::new("set_brightness", json!({"target": target, "percent": percent}))
        }
        IntentAction::SetTemperature { target, celsius } => {
            ToolCall::new("set_temperature", json!({"target": target, "celsius": celsius}))
        }
        IntentAction::SetCover { target, open } => {
            ToolCall::new("set_cover", json!({"target": target, "open": open}))
        }
        IntentAction::ActivateScene { name } => {
            ToolCall::new("activate_scene", json!({"name": name}))
        }
        IntentAction::QueryState { target, what } => {
            let aspect = match what {
                QueryKind::Temperature => "temperature",
                QueryKind::OnState => "on_state",
            };
            ToolCall::new("query_state", json!({"target": target, "what": aspect}))
        }
    }
}

/// The built-in cave-home tool surface advertised to the model. Each is a
/// function whose JSON-Schema mirrors the [`IntentAction`] it maps to.
#[must_use]
pub fn builtin_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec::function(
            "set_light",
            "Turn a light or a whole room's lights on or off.",
            json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "device or room name"},
                    "on": {"type": "boolean"}
                },
                "required": ["target", "on"]
            }),
        ),
        ToolSpec::function(
            "set_brightness",
            "Set a light's brightness as a percentage from 0 to 100.",
            json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"},
                    "percent": {"type": "integer", "minimum": 0, "maximum": 100}
                },
                "required": ["target", "percent"]
            }),
        ),
        ToolSpec::function(
            "set_temperature",
            "Set a room's target temperature in whole degrees Celsius.",
            json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"},
                    "celsius": {"type": "integer"}
                },
                "required": ["target", "celsius"]
            }),
        ),
        ToolSpec::function(
            "set_cover",
            "Open or close a blind, curtain or garage door.",
            json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"},
                    "open": {"type": "boolean"}
                },
                "required": ["target", "open"]
            }),
        ),
        ToolSpec::function(
            "activate_scene",
            "Activate a named scene, e.g. 'movie night'.",
            json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }),
        ),
        ToolSpec::function(
            "query_state",
            "Read back the current state of something (temperature or on/off).",
            json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"},
                    "what": {"type": "string", "enum": ["temperature", "on_state"]}
                },
                "required": ["target", "what"]
            }),
        ),
    ]
}

/// A set of tools the model may call, with argument validation.
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    specs: Vec<ToolSpec>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            specs: builtin_tools(),
        }
    }
}

impl ToolRegistry {
    /// A registry over the given specs.
    #[must_use]
    pub const fn new(specs: Vec<ToolSpec>) -> Self {
        Self { specs }
    }

    /// The specs to advertise in a chat request.
    #[must_use]
    pub fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    /// Look up a tool by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.specs.iter().find(|s| s.function.name == name)
    }

    /// Validate a model-emitted tool call: the tool must exist, the arguments
    /// must be an object, and every `required` schema key must be present.
    ///
    /// # Errors
    /// [`JarvisError::UnknownTool`] or [`JarvisError::ToolArguments`].
    pub fn validate(&self, call: &ToolCall) -> Result<()> {
        let spec = self
            .get(call.name())
            .ok_or_else(|| JarvisError::UnknownTool(call.name().to_string()))?;
        let args = call.arguments();
        let obj = args.as_object().ok_or_else(|| JarvisError::ToolArguments {
            tool: call.name().to_string(),
            reason: "arguments must be a JSON object".into(),
        })?;
        if let Some(Value::Array(required)) = spec.function.parameters.get("required") {
            for key in required.iter().filter_map(Value::as_str) {
                if !obj.contains_key(key) {
                    return Err(JarvisError::ToolArguments {
                        tool: call.name().to_string(),
                        reason: format!("missing required argument '{key}'"),
                    });
                }
            }
        }
        Ok(())
    }
}

/// Executes a validated tool call against cave-home's services. The production
/// implementation talks to the device crates / MQTT; the suite uses
/// [`MockToolExecutor`].
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Perform `call`, returning a speakable [`ToolResult`].
    ///
    /// # Errors
    /// [`JarvisError::ToolFailed`] if the service rejected the command.
    async fn execute(&self, call: &ToolCall) -> Result<ToolResult>;
}

/// A recording executor for tests: logs every call and returns a scripted
/// result (success with the call name by default).
#[derive(Debug, Default)]
pub struct MockToolExecutor {
    /// Every call executed, in order.
    pub calls: parking_lot::Mutex<Vec<ToolCall>>,
    scripted: parking_lot::Mutex<std::collections::VecDeque<ToolResult>>,
}

impl MockToolExecutor {
    /// An executor that always succeeds.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Script the next result.
    #[must_use]
    pub fn returning(self, result: ToolResult) -> Self {
        self.scripted.lock().push_back(result);
        self
    }

    /// The names of the calls executed, in order.
    #[must_use]
    pub fn call_names(&self) -> Vec<String> {
        self.calls.lock().iter().map(|c| c.name().to_string()).collect()
    }
}

#[async_trait]
impl ToolExecutor for MockToolExecutor {
    async fn execute(&self, call: &ToolCall) -> Result<ToolResult> {
        self.calls.lock().push(call.clone());
        let scripted = self.scripted.lock().pop_front();
        Ok(scripted.unwrap_or_else(|| ToolResult::ok(format!("{} done", call.name()))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_light_intent_maps_to_set_light_tool() {
        let call = intent_to_tool_call(&IntentAction::SetLight {
            target: "kitchen".into(),
            on: true,
        });
        assert_eq!(call.name(), "set_light");
        assert_eq!(call.arguments()["target"], "kitchen");
        assert_eq!(call.arguments()["on"], true);
    }

    #[test]
    fn query_state_maps_aspect() {
        let call = intent_to_tool_call(&IntentAction::QueryState {
            target: "living room".into(),
            what: QueryKind::Temperature,
        });
        assert_eq!(call.name(), "query_state");
        assert_eq!(call.arguments()["what"], "temperature");
    }

    #[test]
    fn registry_advertises_all_builtins() {
        let reg = ToolRegistry::default();
        for name in [
            "set_light",
            "set_brightness",
            "set_temperature",
            "set_cover",
            "activate_scene",
            "query_state",
        ] {
            assert!(reg.get(name).is_some(), "missing tool {name}");
        }
        assert_eq!(reg.specs().len(), 6);
    }

    #[test]
    fn validate_accepts_well_formed_call() {
        let reg = ToolRegistry::default();
        let call = ToolCall::new("set_light", serde_json::json!({"target": "kitchen", "on": true}));
        assert!(reg.validate(&call).is_ok());
    }

    #[test]
    fn validate_rejects_unknown_tool() {
        let reg = ToolRegistry::default();
        let call = ToolCall::new("launch_rocket", serde_json::json!({}));
        assert!(matches!(
            reg.validate(&call).unwrap_err(),
            JarvisError::UnknownTool(_)
        ));
    }

    #[test]
    fn validate_rejects_missing_required_arg() {
        let reg = ToolRegistry::default();
        let call = ToolCall::new("set_light", serde_json::json!({"target": "kitchen"})); // no `on`
        assert!(matches!(
            reg.validate(&call).unwrap_err(),
            JarvisError::ToolArguments { .. }
        ));
    }

    #[tokio::test]
    async fn mock_executor_records_and_defaults_to_ok() {
        let exec = MockToolExecutor::new();
        let call = ToolCall::new("set_light", serde_json::json!({"target": "x", "on": false}));
        let r = exec.execute(&call).await.unwrap();
        assert!(r.ok);
        assert_eq!(exec.call_names(), vec!["set_light".to_string()]);
    }

    #[tokio::test]
    async fn mock_executor_returns_scripted_result() {
        let exec = MockToolExecutor::new().returning(ToolResult::failed("device offline"));
        let call = ToolCall::new("set_cover", serde_json::json!({"target": "garage", "open": true}));
        let r = exec.execute(&call).await.unwrap();
        assert!(!r.ok);
        assert_eq!(r.message, "device offline");
    }
}
