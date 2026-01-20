//! Feature flags and metadata for deepseek-cli.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Lifecycle stage for a feature flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Experimental,
    Beta,
    Stable,
    Deprecated,
    Removed,
}

/// Unique features toggled via configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    /// Enable the default shell tool.
    ShellTool,
    /// Enable background sub-agent tooling.
    Subagents,
    /// Enable web search tool.
    WebSearch,
    /// Enable apply_patch tool.
    ApplyPatch,
    /// Enable MCP tools.
    Mcp,
    /// Enable RLM tools.
    Rlm,
    /// Enable Duo tools.
    Duo,
    /// Enable execpolicy integration/tooling.
    ExecPolicy,
}

impl Feature {
    pub fn key(self) -> &'static str {
        self.info().key
    }

    pub fn stage(self) -> Stage {
        self.info().stage
    }

    pub fn default_enabled(self) -> bool {
        self.info().default_enabled
    }

    fn info(self) -> &'static FeatureSpec {
        FEATURES
            .iter()
            .find(|spec| spec.id == self)
            .unwrap_or_else(|| unreachable!("missing FeatureSpec for {:?}", self))
    }
}

/// Holds the effective set of enabled features.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Features {
    enabled: BTreeSet<Feature>,
}

impl Features {
    /// Starts with built-in defaults.
    pub fn with_defaults() -> Self {
        let mut set = BTreeSet::new();
        for spec in FEATURES {
            if spec.default_enabled {
                set.insert(spec.id);
            }
        }
        Self { enabled: set }
    }

    pub fn enabled(&self, feature: Feature) -> bool {
        self.enabled.contains(&feature)
    }

    pub fn enable(&mut self, feature: Feature) -> &mut Self {
        self.enabled.insert(feature);
        self
    }

    pub fn disable(&mut self, feature: Feature) -> &mut Self {
        self.enabled.remove(&feature);
        self
    }

    pub fn apply_map(&mut self, entries: &BTreeMap<String, bool>) {
        for (key, enabled) in entries {
            if let Some(feature) = feature_from_key(key) {
                if *enabled {
                    self.enable(feature);
                } else {
                    self.disable(feature);
                }
            }
        }
    }

    pub fn enabled_features(&self) -> Vec<Feature> {
        let mut list: Vec<_> = self.enabled.iter().copied().collect();
        list.sort();
        list
    }
}

/// Keys accepted in `[features]` tables.
pub fn is_known_feature_key(key: &str) -> bool {
    FEATURES.iter().any(|spec| spec.key == key)
}

pub fn feature_from_key(key: &str) -> Option<Feature> {
    FEATURES
        .iter()
        .find(|spec| spec.key == key)
        .map(|spec| spec.id)
}

pub fn feature_spec_by_key(key: &str) -> Option<&'static FeatureSpec> {
    FEATURES.iter().find(|spec| spec.key == key)
}

/// Deserializable features table for TOML.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct FeaturesToml {
    #[serde(flatten)]
    pub entries: BTreeMap<String, bool>,
}

/// Single registry of all feature definitions.
#[derive(Debug, Clone, Copy)]
pub struct FeatureSpec {
    pub id: Feature,
    pub key: &'static str,
    pub stage: Stage,
    pub default_enabled: bool,
}

pub const FEATURES: &[FeatureSpec] = &[
    FeatureSpec {
        id: Feature::ShellTool,
        key: "shell_tool",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::Subagents,
        key: "subagents",
        stage: Stage::Experimental,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::WebSearch,
        key: "web_search",
        stage: Stage::Experimental,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::ApplyPatch,
        key: "apply_patch",
        stage: Stage::Experimental,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::Mcp,
        key: "mcp",
        stage: Stage::Experimental,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::Rlm,
        key: "rlm",
        stage: Stage::Experimental,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::Duo,
        key: "duo",
        stage: Stage::Experimental,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::ExecPolicy,
        key: "exec_policy",
        stage: Stage::Experimental,
        default_enabled: true,
    },
];
