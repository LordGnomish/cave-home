// SPDX-License-Identifier: Apache-2.0
//! A small, std-only nested value tree and the helm-controller values-merge.
//!
//! helm-controller (and Helm itself) layer values from several sources before
//! handing the merged result to the chart's templates. The documented
//! precedence — lowest wins first, highest overrides — is:
//!
//! ```text
//! chart defaults  <  spec.valuesContent  <  HelmChartConfig overlay  <  spec.set
//! ```
//!
//! Helm's merge is a *deep* merge for maps (Go: `mergeMaps`): keys present in a
//! higher-precedence layer override the lower one; keys absent in the higher
//! layer are inherited from the lower one; nested maps recurse. Scalars and
//! arrays replace wholesale (Helm does **not** concatenate arrays on merge).
//!
//! `set`-style overrides additionally honour an explicit `null` to *remove* a
//! key from the merged result — this mirrors `helm upgrade --set key=null`,
//! which deletes the key rather than setting it to a null scalar.
//!
//! Spec sources (public, Apache-2.0-compatible documentation):
//! * Helm docs — "Values Files" & "The Format and Limitations of --set".
//! * `helm.sh/docs/chart_template_guide/values_files`.
//! * k3s-io/helm-controller `HelmChart` CRD reference (public CRD docs).

use std::collections::BTreeMap;

/// A minimal nested value tree, modelling Helm values without a YAML dependency.
///
/// `Object` uses a `BTreeMap` so iteration order — and therefore the canonical
/// serialization used for hashing — is deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    /// Numbers are kept as their source text to avoid float-equality and
    /// precision surprises; Helm treats `--set` numbers textually too.
    Number(String),
    String(String),
    /// An ordered list of values.
    Array(Vec<Self>),
    /// A keyed map; keys are sorted for determinism.
    Object(BTreeMap<String, Self>),
}

impl Value {
    /// Construct an empty object — the identity element for [`Value::deep_merge`].
    #[must_use]
    pub const fn object() -> Self {
        Self::Object(BTreeMap::new())
    }

    /// Insert a key into an object, creating the object if `self` was not one.
    ///
    /// Returns `self` for builder-style chaining.
    #[must_use]
    pub fn with(mut self, key: &str, value: Self) -> Self {
        if let Self::Object(map) = &mut self {
            map.insert(key.to_string(), value);
            self
        } else {
            let mut map = BTreeMap::new();
            map.insert(key.to_string(), value);
            Self::Object(map)
        }
    }

    /// Is this an object (map)?
    #[must_use]
    pub const fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    /// Borrow the object map, if this value is one.
    #[must_use]
    pub const fn as_object(&self) -> Option<&BTreeMap<String, Self>> {
        if let Self::Object(map) = self {
            Some(map)
        } else {
            None
        }
    }

    /// Deep-merge `higher` into `self` (`self` is the lower-precedence layer).
    ///
    /// Semantics, matching Helm's `mergeMaps` plus `--set` null-removal:
    /// * two objects → recurse key-by-key;
    /// * a higher `Null` → *removes* the key (so callers should merge layers
    ///   then read the result; a surviving `Null` only appears at the root);
    /// * any other higher value → replaces the lower value wholesale
    ///   (scalars and arrays do not merge element-wise).
    #[must_use]
    pub fn deep_merge(self, higher: Self) -> Self {
        match (self, higher) {
            (Self::Object(mut lo), Self::Object(hi)) => {
                for (k, hv) in hi {
                    if matches!(hv, Self::Null) {
                        // Explicit null removes the key (helm --set key=null).
                        lo.remove(&k);
                        continue;
                    }
                    let merged = match lo.remove(&k) {
                        Some(lv) => lv.deep_merge(hv),
                        None => hv,
                    };
                    lo.insert(k, merged);
                }
                Self::Object(lo)
            }
            // A higher null at a non-object position clears the value.
            (_, Self::Null) => Self::Null,
            // Any non-object higher layer replaces the lower one wholesale.
            (_, higher) => higher,
        }
    }

    /// Look up a dotted path (`a.b.c`) in an object tree.
    #[must_use]
    pub fn get_path(&self, path: &str) -> Option<&Self> {
        let mut cur = self;
        for seg in path.split('.') {
            cur = cur.as_object()?.get(seg)?;
        }
        Some(cur)
    }

    /// Canonical, stable string serialization used as hash input.
    ///
    /// Object keys are emitted in `BTreeMap` (sorted) order, so semantically
    /// equal trees always produce byte-identical output regardless of insert
    /// order. This is *not* YAML — it is an internal canonical form.
    #[must_use]
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        self.write_canonical(&mut out);
        out
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Number(n) => {
                out.push('#');
                out.push_str(n);
            }
            Self::String(s) => {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
            Self::Array(items) => {
                out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    it.write_canonical(out);
                }
                out.push(']');
            }
            Self::Object(map) => {
                out.push('{');
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(k);
                    out.push(':');
                    v.write_canonical(out);
                }
                out.push('}');
            }
        }
    }
}

/// Merge the full helm-controller layer stack in documented precedence order.
///
/// Lowest first; each subsequent layer overrides. Any layer may be `None`.
#[must_use]
pub fn merge_layers(
    chart_defaults: Option<Value>,
    values_content: Option<Value>,
    config_overlay: Option<Value>,
    set_values: Option<Value>,
) -> Value {
    let mut acc = chart_defaults.unwrap_or_else(Value::object);
    if let Some(v) = values_content {
        acc = acc.deep_merge(v);
    }
    if let Some(v) = config_overlay {
        acc = acc.deep_merge(v);
    }
    if let Some(v) = set_values {
        acc = acc.deep_merge(v);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::String(v.to_string())
    }

    #[test]
    fn higher_layer_overrides_scalar() {
        let lo = Value::object().with("image", s("nginx:1.0"));
        let hi = Value::object().with("image", s("nginx:2.0"));
        let m = lo.deep_merge(hi);
        assert_eq!(m.get_path("image"), Some(&s("nginx:2.0")));
    }

    #[test]
    fn lower_layer_keys_survive_when_absent_in_higher() {
        let lo = Value::object()
            .with("image", s("nginx"))
            .with("replicas", Value::Number("1".into()));
        let hi = Value::object().with("replicas", Value::Number("3".into()));
        let m = lo.deep_merge(hi);
        // image inherited from lower, replicas overridden by higher.
        assert_eq!(m.get_path("image"), Some(&s("nginx")));
        assert_eq!(m.get_path("replicas"), Some(&Value::Number("3".into())));
    }

    #[test]
    fn nested_objects_merge_recursively() {
        let lo = Value::object().with(
            "resources",
            Value::object()
                .with("cpu", s("100m"))
                .with("memory", s("128Mi")),
        );
        let hi = Value::object().with("resources", Value::object().with("cpu", s("250m")));
        let m = lo.deep_merge(hi);
        // cpu overridden, memory preserved through nested merge.
        assert_eq!(m.get_path("resources.cpu"), Some(&s("250m")));
        assert_eq!(m.get_path("resources.memory"), Some(&s("128Mi")));
    }

    #[test]
    fn arrays_replace_not_concatenate() {
        let lo = Value::object().with("args", Value::Array(vec![s("--a"), s("--b")]));
        let hi = Value::object().with("args", Value::Array(vec![s("--c")]));
        let m = lo.deep_merge(hi);
        assert_eq!(
            m.get_path("args"),
            Some(&Value::Array(vec![s("--c")])),
            "Helm replaces arrays wholesale, it does not concatenate"
        );
    }

    #[test]
    fn explicit_null_removes_key() {
        let lo = Value::object()
            .with("keep", s("yes"))
            .with("drop", s("old"));
        let hi = Value::object().with("drop", Value::Null);
        let m = lo.deep_merge(hi);
        assert_eq!(m.get_path("keep"), Some(&s("yes")));
        assert_eq!(m.get_path("drop"), None, "--set drop=null deletes the key");
    }

    #[test]
    fn null_removes_nested_key_only() {
        let lo = Value::object().with(
            "svc",
            Value::object().with("a", s("1")).with("b", s("2")),
        );
        let hi = Value::object().with("svc", Value::object().with("b", Value::Null));
        let m = lo.deep_merge(hi);
        assert_eq!(m.get_path("svc.a"), Some(&s("1")));
        assert_eq!(m.get_path("svc.b"), None);
    }

    #[test]
    fn full_precedence_set_beats_config_beats_content_beats_defaults() {
        let defaults = Value::object()
            .with("tier", s("default"))
            .with("img", s("d"));
        let content = Value::object().with("img", s("c")).with("env", s("prod"));
        let config = Value::object().with("img", s("cfg"));
        let set = Value::object().with("img", s("set"));
        let m = merge_layers(
            Some(defaults),
            Some(content),
            Some(config),
            Some(set),
        );
        // img is set in every layer: highest (set) wins.
        assert_eq!(m.get_path("img"), Some(&s("set")));
        // tier only in defaults, env only in content: both survive.
        assert_eq!(m.get_path("tier"), Some(&s("default")));
        assert_eq!(m.get_path("env"), Some(&s("prod")));
    }

    #[test]
    fn merge_layers_handles_all_none_but_defaults() {
        let m = merge_layers(Some(Value::object().with("k", s("v"))), None, None, None);
        assert_eq!(m.get_path("k"), Some(&s("v")));
    }

    #[test]
    fn config_overlay_beats_values_content() {
        let content = Value::object().with("replicas", Value::Number("1".into()));
        let config = Value::object().with("replicas", Value::Number("5".into()));
        let m = merge_layers(None, Some(content), Some(config), None);
        assert_eq!(m.get_path("replicas"), Some(&Value::Number("5".into())));
    }

    #[test]
    fn canonical_is_order_independent() {
        let a = Value::object().with("b", s("2")).with("a", s("1"));
        let b = Value::object().with("a", s("1")).with("b", s("2"));
        assert_eq!(a.canonical(), b.canonical());
    }

    #[test]
    fn canonical_distinguishes_number_from_string() {
        let n = Value::object().with("x", Value::Number("1".into()));
        let st = Value::object().with("x", s("1"));
        assert_ne!(n.canonical(), st.canonical());
    }
}
