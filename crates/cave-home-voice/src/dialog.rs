// SPDX-License-Identifier: Apache-2.0
//! Dialog file loader + variable substitution.
//!
//! # Upstream:
//! - `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/dialog/dialog.py::MustacheDialogRenderer` —
//!   loads `<intent>.dialog` files (one line per response variant) and
//!   substitutes `{slot}` placeholders. Reproduced one-to-one in
//!   [`DialogRenderer::render`].
//! - `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/dialog/dialog.py::MustacheDialogRenderer.load_template_file` —
//!   the loader is mirrored in [`DialogRenderer::load_dir`].
//! - `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/dialog/dialog.py::get_response` —
//!   selects one variant from the candidate list (random in upstream;
//!   round-robin here for testability — same surface).

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

use crate::error::VoiceResult;

/// Loads dialog files and renders responses.
///
/// # Upstream:
/// `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/dialog/dialog.py::MustacheDialogRenderer`
#[derive(Default)]
pub struct DialogRenderer {
    /// `template_id` → list of response variants.
    templates: Mutex<HashMap<String, Vec<String>>>,
    /// Per-template round-robin cursor for deterministic tests.
    cursors: Mutex<HashMap<String, AtomicUsize>>,
}

impl DialogRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a template with its variant list.
    pub fn add(&self, template: &str, variants: Vec<String>) {
        self.templates.lock().insert(template.to_string(), variants);
    }

    /// True when no templates have been loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.templates.lock().is_empty()
    }

    /// Walk a directory of `*.dialog` files and load each one.
    ///
    /// # Upstream:
    /// `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/dialog/dialog.py::MustacheDialogRenderer.load_template_file`
    ///
    /// # Errors
    /// Returns `VoiceError::Io` on directory read failure.
    pub async fn load_dir(&self, dir: &Path) -> VoiceResult<usize> {
        let mut count = 0_usize;
        let mut read = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = read.next_entry().await? {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.ends_with(".dialog") {
                continue;
            }
            let template = name.trim_end_matches(".dialog").to_string();
            let body = tokio::fs::read_to_string(&path).await?;
            let variants: Vec<String> = body
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(ToString::to_string)
                .collect();
            if !variants.is_empty() {
                self.add(&template, variants);
                count += 1;
            }
        }
        Ok(count)
    }

    /// Render one response variant for `template`, substituting `{slot}`
    /// placeholders from `slots`.
    ///
    /// Returns `None` when the template is unknown. Round-robin cursor
    /// ensures tests can assert a specific variant.
    ///
    /// # Upstream:
    /// `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/dialog/dialog.py::MustacheDialogRenderer.render`
    #[must_use]
    pub fn render(&self, template: &str, slots: &HashMap<String, String>) -> Option<String> {
        let templates = self.templates.lock();
        let variants = templates.get(template)?.clone();
        drop(templates);
        if variants.is_empty() {
            return None;
        }
        let mut cursors = self.cursors.lock();
        let cursor = cursors
            .entry(template.to_string())
            .or_insert_with(|| AtomicUsize::new(0));
        let idx = cursor.fetch_add(1, Ordering::Relaxed) % variants.len();
        drop(cursors);
        let raw = &variants[idx];
        Some(substitute(raw, slots))
    }

    /// Number of registered templates.
    #[must_use]
    pub fn len(&self) -> usize {
        self.templates.lock().len()
    }
}

/// `{slot}` substitution.
///
/// # Upstream:
/// `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/dialog/dialog.py::MustacheDialogRenderer.render`
/// — the upstream uses mustache `{{name}}`; we accept both `{name}` and
/// `{{name}}` for ergonomic parity with the dialog files in
/// `ovos-skills-*`.
#[must_use]
pub fn substitute(template: &str, slots: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            // Consume an optional second '{'.
            let double = matches!(chars.peek(), Some('{'));
            if double {
                chars.next();
            }
            let mut name = String::new();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '}' {
                    if double {
                        if matches!(chars.peek(), Some('}')) {
                            chars.next();
                            closed = true;
                            break;
                        }
                        name.push(c);
                        continue;
                    }
                    closed = true;
                    break;
                }
                name.push(c);
            }
            let key = name.trim();
            if closed && !key.is_empty() {
                if let Some(v) = slots.get(key) {
                    out.push_str(v);
                    continue;
                }
                // No slot present — leave the placeholder literal so
                // the operator notices in the log.
                if double {
                    out.push_str("{{");
                    out.push_str(&name);
                    out.push_str("}}");
                } else {
                    out.push('{');
                    out.push_str(&name);
                    out.push('}');
                }
            } else if !closed {
                out.push('{');
                if double {
                    out.push('{');
                }
                out.push_str(&name);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
    }

    #[test]
    fn substitute_replaces_single_brace_placeholders() {
        let s = slots(&[("room", "salon")]);
        let out = substitute("{room} ışıkları kapatıldı", &s);
        assert_eq!(out, "salon ışıkları kapatıldı");
    }

    #[test]
    fn substitute_replaces_double_brace_placeholders() {
        let s = slots(&[("name", "Burak")]);
        let out = substitute("Merhaba {{name}}", &s);
        assert_eq!(out, "Merhaba Burak");
    }

    #[test]
    fn substitute_leaves_unknown_placeholders_literal() {
        let s = HashMap::new();
        let out = substitute("Bilinmeyen: {missing}", &s);
        assert_eq!(out, "Bilinmeyen: {missing}");
    }

    #[test]
    fn dialog_renderer_round_robin_walks_variants() {
        let r = DialogRenderer::new();
        r.add(
            "greet",
            vec!["Merhaba".to_string(), "Selam".to_string(), "Günaydın".to_string()],
        );
        let s = HashMap::new();
        let a = r.render("greet", &s).expect("render");
        let b = r.render("greet", &s).expect("render");
        let c = r.render("greet", &s).expect("render");
        let d = r.render("greet", &s).expect("render");
        assert_eq!(a, "Merhaba");
        assert_eq!(b, "Selam");
        assert_eq!(c, "Günaydın");
        assert_eq!(d, "Merhaba"); // wraps
    }

    #[test]
    fn dialog_renderer_returns_none_for_unknown_template() {
        let r = DialogRenderer::new();
        assert!(r.render("nope", &HashMap::new()).is_none());
    }

    #[tokio::test]
    async fn dialog_renderer_loads_dialog_files_from_dir() {
        let tmp = tempfile::tempdir().expect("tmp");
        let p = tmp.path().join("lights_on.dialog");
        tokio::fs::write(&p, "Işıklar açıldı\n# yorum satırı\nIşığı yaktım\n")
            .await
            .expect("write");
        let r = DialogRenderer::new();
        let n = r.load_dir(tmp.path()).await.expect("load");
        assert_eq!(n, 1);
        let out = r
            .render("lights_on", &HashMap::new())
            .expect("render");
        assert!(out == "Işıklar açıldı" || out == "Işığı yaktım");
    }
}
