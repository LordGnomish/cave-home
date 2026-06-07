// SPDX-License-Identifier: Apache-2.0
//! Patch application over the value tree.
//!
//! Implements two of the documented apiserver patch strategies:
//! - **JSON Merge Patch** (RFC 7396), used for `Content-Type:
//!   application/merge-patch+json`.
//! - **JSON Patch** (RFC 6902), used for `application/json-patch+json`.
//!
//! Behavioural reference: RFC 7396 §2 and RFC 6902 §4. Clean-room
//! reimplementation of the documented algorithms. Strategic-merge-patch (the
//! k8s-specific list-merge strategy) is deferred (see `parity.manifest.toml`).

use crate::json::Value;
use crate::status::Status;

/// Apply an RFC 7396 JSON Merge Patch: recursively merge objects; a `null`
/// value deletes the key; any non-object patch replaces the target wholesale.
#[must_use]
pub fn apply_merge_patch(target: &Value, patch: &Value) -> Value {
    match patch {
        Value::Object(patch_obj) => {
            // If the target is not an object, RFC 7396 starts from an empty one.
            let mut out = match target {
                Value::Object(t) => t.clone(),
                _ => std::collections::BTreeMap::new(),
            };
            for (k, v) in patch_obj {
                if v.is_null() {
                    out.remove(k);
                } else {
                    let merged = match out.get(k) {
                        Some(existing) => apply_merge_patch(existing, v),
                        None => apply_merge_patch(&Value::Null, v),
                    };
                    out.insert(k.clone(), merged);
                }
            }
            Value::Object(out)
        }
        // Non-object patch replaces the whole document.
        other => other.clone(),
    }
}

/// One RFC 6902 operation.
#[derive(Clone, Debug, PartialEq)]
pub enum PatchOp {
    /// Add (or replace, for object members) the value at `path`.
    Add { path: String, value: Value },
    /// Remove the value at `path`.
    Remove { path: String },
    /// Replace the value at `path` (which must exist).
    Replace { path: String, value: Value },
    /// Test that the value at `path` equals `value`.
    Test { path: String, value: Value },
}

/// Apply an RFC 6902 JSON Patch (a sequence of operations) to `target`,
/// returning the patched document.
///
/// # Errors
/// Returns an `Invalid` [`Status`] if any operation cannot be applied (missing
/// path for replace/remove, failed test, malformed pointer).
pub fn apply_json_patch(target: &Value, ops: &[PatchOp]) -> Result<Value, Status> {
    let mut doc = target.clone();
    for op in ops {
        match op {
            PatchOp::Add { path, value } => set_at(&mut doc, path, value.clone(), true)?,
            PatchOp::Replace { path, value } => set_at(&mut doc, path, value.clone(), false)?,
            PatchOp::Remove { path } => {
                remove_at(&mut doc, path)?;
            }
            PatchOp::Test { path, value } => {
                let actual = resolve(&doc, path)
                    .ok_or_else(|| Status::invalid(format!("test path {path:?} not found")))?;
                if actual != value {
                    return Err(Status::invalid(format!("test failed at {path:?}")));
                }
            }
        }
    }
    Ok(doc)
}

/// Parse an RFC 6901 JSON pointer into unescaped tokens. The empty string
/// (whole document) yields an empty token list.
fn parse_pointer(path: &str) -> Result<Vec<String>, Status> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let rest = path
        .strip_prefix('/')
        .ok_or_else(|| Status::invalid(format!("JSON pointer must start with '/': {path:?}")))?;
    Ok(rest
        .split('/')
        .map(|t| t.replace("~1", "/").replace("~0", "~"))
        .collect())
}

fn resolve<'a>(doc: &'a Value, path: &str) -> Option<&'a Value> {
    let tokens = parse_pointer(path).ok()?;
    let mut cur = doc;
    for t in tokens {
        cur = match cur {
            Value::Object(m) => m.get(&t)?,
            Value::Array(a) => {
                let i: usize = t.parse().ok()?;
                a.get(i)?
            }
            _ => return None,
        };
    }
    Some(cur)
}

/// Set the value at `path`. When `allow_create` is false (replace), the leaf
/// must already exist.
fn set_at(doc: &mut Value, path: &str, value: Value, allow_create: bool) -> Result<(), Status> {
    let tokens = parse_pointer(path)?;
    if tokens.is_empty() {
        *doc = value;
        return Ok(());
    }
    let (last, parents) = tokens.split_last().ok_or_else(|| Status::invalid("empty pointer"))?;
    let mut cur = doc;
    for t in parents {
        cur = descend_mut(cur, t)
            .ok_or_else(|| Status::invalid(format!("path segment {t:?} not found in {path:?}")))?;
    }
    match cur {
        Value::Object(m) => {
            if !allow_create && !m.contains_key(last) {
                return Err(Status::invalid(format!("replace target {path:?} does not exist")));
            }
            m.insert(last.clone(), value);
            Ok(())
        }
        Value::Array(a) => {
            if last == "-" {
                if !allow_create {
                    return Err(Status::invalid(format!("cannot replace at append index {path:?}")));
                }
                a.push(value);
                return Ok(());
            }
            let i: usize = last
                .parse()
                .map_err(|_| Status::invalid(format!("bad array index {last:?}")))?;
            if allow_create {
                if i > a.len() {
                    return Err(Status::invalid(format!("array index {i} out of bounds")));
                }
                a.insert(i, value);
            } else {
                let slot = a
                    .get_mut(i)
                    .ok_or_else(|| Status::invalid(format!("replace index {i} out of bounds")))?;
                *slot = value;
            }
            Ok(())
        }
        _ => Err(Status::invalid(format!("cannot set on non-container at {path:?}"))),
    }
}

fn remove_at(doc: &mut Value, path: &str) -> Result<(), Status> {
    let tokens = parse_pointer(path)?;
    let (last, parents) = tokens
        .split_last()
        .ok_or_else(|| Status::invalid("cannot remove whole document"))?;
    let mut cur = doc;
    for t in parents {
        cur = descend_mut(cur, t)
            .ok_or_else(|| Status::invalid(format!("path segment {t:?} not found in {path:?}")))?;
    }
    match cur {
        Value::Object(m) => {
            if m.remove(last).is_none() {
                return Err(Status::invalid(format!("remove target {path:?} does not exist")));
            }
            Ok(())
        }
        Value::Array(a) => {
            let i: usize = last
                .parse()
                .map_err(|_| Status::invalid(format!("bad array index {last:?}")))?;
            if i >= a.len() {
                return Err(Status::invalid(format!("remove index {i} out of bounds")));
            }
            a.remove(i);
            Ok(())
        }
        _ => Err(Status::invalid(format!("cannot remove from non-container at {path:?}"))),
    }
}

fn descend_mut<'a>(cur: &'a mut Value, token: &str) -> Option<&'a mut Value> {
    match cur {
        Value::Object(m) => m.get_mut(token),
        Value::Array(a) => {
            let i: usize = token.parse().ok()?;
            a.get_mut(i)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::obj;

    #[test]
    fn merge_patch_adds_and_overwrites() {
        let target = obj([("a", Value::from(1_i64)), ("b", Value::from("x"))]);
        let patch = obj([("b", Value::from("y")), ("c", Value::from(3_i64))]);
        let out = apply_merge_patch(&target, &patch);
        assert_eq!(out.get("a"), Some(&Value::from(1_i64)));
        assert_eq!(out.get("b"), Some(&Value::from("y")));
        assert_eq!(out.get("c"), Some(&Value::from(3_i64)));
    }

    #[test]
    fn merge_patch_null_deletes_key() {
        let target = obj([("a", Value::from(1_i64)), ("b", Value::from(2_i64))]);
        let patch = obj([("a", Value::Null)]);
        let out = apply_merge_patch(&target, &patch);
        assert!(out.get("a").is_none());
        assert_eq!(out.get("b"), Some(&Value::from(2_i64)));
    }

    #[test]
    fn merge_patch_recurses_into_objects() {
        let target = obj([(
            "metadata",
            obj([("labels", obj([("app", Value::from("nginx"))]))]),
        )]);
        let patch = obj([(
            "metadata",
            obj([("labels", obj([("tier", Value::from("web"))]))]),
        )]);
        let out = apply_merge_patch(&target, &patch);
        let labels = out.pointer("metadata.labels").expect("labels");
        assert_eq!(labels.get("app"), Some(&Value::from("nginx")));
        assert_eq!(labels.get("tier"), Some(&Value::from("web")));
    }

    #[test]
    fn merge_patch_non_object_replaces() {
        let target = obj([("a", Value::from(1_i64))]);
        let out = apply_merge_patch(&target, &Value::from("scalar"));
        assert_eq!(out, Value::from("scalar"));
    }

    #[test]
    fn json_patch_replace_existing() {
        let target = obj([("spec", obj([("replicas", Value::from(1_i64))]))]);
        let ops = vec![PatchOp::Replace {
            path: "/spec/replicas".into(),
            value: Value::from(3_i64),
        }];
        let out = apply_json_patch(&target, &ops).expect("patch");
        assert_eq!(out.pointer("spec.replicas"), Some(&Value::from(3_i64)));
    }

    #[test]
    fn json_patch_replace_missing_is_invalid() {
        let target = obj([("spec", Value::object())]);
        let ops = vec![PatchOp::Replace {
            path: "/spec/replicas".into(),
            value: Value::from(3_i64),
        }];
        let err = apply_json_patch(&target, &ops).expect_err("should fail");
        assert_eq!(err.reason, crate::status::StatusReason::Invalid);
    }

    #[test]
    fn json_patch_add_and_remove() {
        let target = obj([("metadata", obj([("name", Value::from("p"))]))]);
        let ops = vec![PatchOp::Add {
            path: "/metadata/labels".into(),
            value: obj([("a", Value::from("b"))]),
        }];
        let out = apply_json_patch(&target, &ops).expect("add");
        assert_eq!(out.pointer("metadata.labels.a"), Some(&Value::from("b")));

        let remove = vec![PatchOp::Remove {
            path: "/metadata/name".into(),
        }];
        let out2 = apply_json_patch(&out, &remove).expect("remove");
        assert!(out2.pointer("metadata.name").is_none());
    }

    #[test]
    fn json_patch_array_append_and_index() {
        let target = obj([("items", Value::Array(vec![Value::from(1_i64)]))]);
        let ops = vec![
            PatchOp::Add {
                path: "/items/-".into(),
                value: Value::from(2_i64),
            },
            PatchOp::Add {
                path: "/items/0".into(),
                value: Value::from(0_i64),
            },
        ];
        let out = apply_json_patch(&target, &ops).expect("patch");
        assert_eq!(
            out.get("items").and_then(Value::as_array),
            Some([Value::from(0_i64), Value::from(1_i64), Value::from(2_i64)].as_slice())
        );
    }

    #[test]
    fn json_patch_test_passes_then_fails() {
        let target = obj([("a", Value::from(1_i64))]);
        let ok = vec![PatchOp::Test {
            path: "/a".into(),
            value: Value::from(1_i64),
        }];
        assert!(apply_json_patch(&target, &ok).is_ok());
        let bad = vec![PatchOp::Test {
            path: "/a".into(),
            value: Value::from(2_i64),
        }];
        assert!(apply_json_patch(&target, &bad).is_err());
    }

    #[test]
    fn pointer_unescapes_tilde_and_slash() {
        let target = obj([("a/b~c", Value::from(1_i64))]);
        let ops = vec![PatchOp::Replace {
            path: "/a~1b~0c".into(),
            value: Value::from(2_i64),
        }];
        let out = apply_json_patch(&target, &ops).expect("patch");
        assert_eq!(out.get("a/b~c"), Some(&Value::from(2_i64)));
    }

    #[test]
    fn whole_document_replace_with_empty_pointer() {
        let target = obj([("a", Value::from(1_i64))]);
        let ops = vec![PatchOp::Add {
            path: String::new(),
            value: Value::from("whole"),
        }];
        let out = apply_json_patch(&target, &ops).expect("patch");
        assert_eq!(out, Value::from("whole"));
    }

    // --- ops_from_json (decode an RFC 6902 op array from a request body) -----

    #[test]
    fn ops_from_json_decodes_all_op_kinds() {
        let doc = crate::json::parse(
            r#"[
                {"op":"add","path":"/metadata/labels/x","value":"1"},
                {"op":"replace","path":"/spec/replicas","value":3},
                {"op":"remove","path":"/status"},
                {"op":"test","path":"/kind","value":"Pod"}
            ]"#,
        )
        .expect("json");
        let ops = ops_from_json(&doc).expect("ops");
        assert_eq!(ops.len(), 4);
        assert_eq!(
            ops[0],
            PatchOp::Add { path: "/metadata/labels/x".into(), value: Value::from("1") }
        );
        assert_eq!(
            ops[1],
            PatchOp::Replace { path: "/spec/replicas".into(), value: Value::from(3_i64) }
        );
        assert_eq!(ops[2], PatchOp::Remove { path: "/status".into() });
        assert_eq!(
            ops[3],
            PatchOp::Test { path: "/kind".into(), value: Value::from("Pod") }
        );
    }

    #[test]
    fn ops_from_json_rejects_non_array() {
        assert!(ops_from_json(&obj([("op", Value::from("add"))])).is_err());
    }

    #[test]
    fn ops_from_json_rejects_unknown_op_and_missing_path() {
        assert!(ops_from_json(&Value::Array(vec![obj([
            ("op", Value::from("frobnicate")),
            ("path", Value::from("/x")),
        ])]))
        .is_err());
        assert!(ops_from_json(&Value::Array(vec![obj([("op", Value::from("add"))])])).is_err());
    }
}
