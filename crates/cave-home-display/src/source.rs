//! The input-source and installed-app model.
//!
//! A TV has a fixed set of physical inputs it was built with (HDMI ports, the
//! built-in tuner) and a changeable set of installed streaming apps. The
//! household selects a source by name; the [`crate::machine`] rejects a request
//! for a source the TV does not have, and rejects launching an app that is not
//! installed. App launching is further gated by an [`AppCapability`] flag so a
//! TV that simply cannot run apps (a plain HDMI display) reports that honestly.

/// A selectable input, identified by a short stable id (`"hdmi1"`, `"tv"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Stable identifier used in commands (lower-kebab, e.g. `"hdmi1"`).
    id: String,
    /// Grandma-friendly display name (e.g. `"HDMI 1"`, `"Live TV"`).
    name: String,
}

impl Source {
    /// A new source from an id and a display name.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }

    /// The stable id used in [`crate::MediaCommand::SelectSource`].
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The household-facing display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A streaming app the TV has installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Stable identifier used in commands (e.g. `"netflix"`).
    id: String,
    /// Grandma-friendly display name (e.g. `"Netflix"`).
    name: String,
}

impl App {
    /// A new app from an id and a display name.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }

    /// The stable id used in [`crate::MediaCommand::LaunchApp`].
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The household-facing display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Whether this TV can run streaming apps at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCapability {
    /// A smart TV: can launch installed apps.
    Smart,
    /// A plain display: inputs only, no apps.
    InputsOnly,
}

/// The fixed inputs and installed apps a particular TV has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceCatalog {
    sources: Vec<Source>,
    apps: Vec<App>,
    app_capability: Option<AppCapability>,
}

impl SourceCatalog {
    /// An empty catalog with no inputs and no apps (capability defaults to
    /// inputs-only until apps are added).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sources: Vec::new(),
            apps: Vec::new(),
            app_capability: None,
        }
    }

    /// Build a catalog from a list of inputs.
    #[must_use]
    pub fn with_sources(sources: Vec<Source>) -> Self {
        Self {
            sources,
            apps: Vec::new(),
            app_capability: None,
        }
    }

    /// Add an input to the catalog (builder style).
    #[must_use]
    pub fn add_source(mut self, source: Source) -> Self {
        self.sources.push(source);
        self
    }

    /// Add an installed app to the catalog (builder style). Adding an app marks
    /// the TV as [`AppCapability::Smart`] unless a capability was set explicitly.
    #[must_use]
    pub fn add_app(mut self, app: App) -> Self {
        self.apps.push(app);
        self
    }

    /// Declare the app capability of this TV explicitly.
    #[must_use]
    pub const fn with_app_capability(mut self, capability: AppCapability) -> Self {
        self.app_capability = Some(capability);
        self
    }

    /// The effective app capability: explicit if set, otherwise [`Smart`] when
    /// any app is installed, otherwise [`InputsOnly`].
    ///
    /// [`Smart`]: AppCapability::Smart
    /// [`InputsOnly`]: AppCapability::InputsOnly
    #[must_use]
    pub fn app_capability(&self) -> AppCapability {
        match self.app_capability {
            Some(c) => c,
            None if self.apps.is_empty() => AppCapability::InputsOnly,
            None => AppCapability::Smart,
        }
    }

    /// All inputs, in declared order.
    #[must_use]
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// All installed apps, in declared order.
    #[must_use]
    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    /// Look up an input by id, if the TV has it.
    #[must_use]
    pub fn find_source(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.id() == id)
    }

    /// Look up an installed app by id, if present.
    #[must_use]
    pub fn find_app(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|a| a.id() == id)
    }

    /// A typical smart-TV catalog: three HDMI ports, the built-in tuner, and a
    /// couple of common apps. Useful for examples and tests.
    #[must_use]
    pub fn typical_smart_tv() -> Self {
        Self {
            sources: vec![
                Source::new("tv", "Live TV"),
                Source::new("hdmi1", "HDMI 1"),
                Source::new("hdmi2", "HDMI 2"),
                Source::new("hdmi3", "HDMI 3"),
            ],
            apps: vec![
                App::new("netflix", "Netflix"),
                App::new("youtube", "YouTube"),
            ],
            app_capability: Some(AppCapability::Smart),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_source_only_matches_known_inputs() {
        let cat = SourceCatalog::typical_smart_tv();
        assert!(cat.find_source("hdmi2").is_some());
        assert!(cat.find_source("hdmi9").is_none(), "unknown input not found");
        assert!(cat.find_source("scart").is_none());
    }

    #[test]
    fn find_app_only_matches_installed_apps() {
        let cat = SourceCatalog::typical_smart_tv();
        assert!(cat.find_app("netflix").is_some());
        assert!(cat.find_app("disney").is_none(), "uninstalled app not found");
    }

    #[test]
    fn empty_catalog_is_inputs_only() {
        assert_eq!(SourceCatalog::new().app_capability(), AppCapability::InputsOnly);
    }

    #[test]
    fn installing_an_app_makes_a_tv_smart() {
        let cat = SourceCatalog::new().add_app(App::new("netflix", "Netflix"));
        assert_eq!(cat.app_capability(), AppCapability::Smart);
    }

    #[test]
    fn explicit_inputs_only_overrides_app_presence() {
        let cat = SourceCatalog::new()
            .add_app(App::new("netflix", "Netflix"))
            .with_app_capability(AppCapability::InputsOnly);
        assert_eq!(cat.app_capability(), AppCapability::InputsOnly);
    }

    #[test]
    fn builder_accumulates_sources_in_order() {
        let cat = SourceCatalog::new()
            .add_source(Source::new("hdmi1", "HDMI 1"))
            .add_source(Source::new("hdmi2", "HDMI 2"));
        assert_eq!(cat.sources().len(), 2);
        assert_eq!(cat.sources()[0].name(), "HDMI 1");
    }
}
