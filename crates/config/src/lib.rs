use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
pub use deepseek_secrets::Secrets;
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE_NAME: &str = "config.toml";
const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-pro";
const DEFAULT_NVIDIA_NIM_MODEL: &str = "deepseek-ai/deepseek-v4-pro";
const DEFAULT_NVIDIA_NIM_FLASH_MODEL: &str = "deepseek-ai/deepseek-v4-flash";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4.1";
const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_NVIDIA_NIM_BASE_URL: &str = "https://integrate.api.nvidia.com/v1";
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENROUTER_MODEL: &str = "deepseek/deepseek-v4-pro";
const DEFAULT_OPENROUTER_FLASH_MODEL: &str = "deepseek/deepseek-v4-flash";
const DEFAULT_NOVITA_MODEL: &str = "deepseek/deepseek-v4-pro";
const DEFAULT_NOVITA_FLASH_MODEL: &str = "deepseek/deepseek-v4-flash";
const DEFAULT_FIREWORKS_MODEL: &str = "accounts/fireworks/models/deepseek-v4-pro";
const DEFAULT_SGLANG_MODEL: &str = "deepseek-ai/DeepSeek-V4-Pro";
const DEFAULT_SGLANG_FLASH_MODEL: &str = "deepseek-ai/DeepSeek-V4-Flash";
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_NOVITA_BASE_URL: &str = "https://api.novita.ai/v1";
const DEFAULT_FIREWORKS_BASE_URL: &str = "https://api.fireworks.ai/inference/v1";
const DEFAULT_SGLANG_BASE_URL: &str = "http://localhost:30000/v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    #[default]
    Deepseek,
    NvidiaNim,
    Openai,
    Openrouter,
    Novita,
    Fireworks,
    Sglang,
}

impl ProviderKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deepseek => "deepseek",
            Self::NvidiaNim => "nvidia-nim",
            Self::Openai => "openai",
            Self::Openrouter => "openrouter",
            Self::Novita => "novita",
            Self::Fireworks => "fireworks",
            Self::Sglang => "sglang",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "deepseek" | "deep-seek" => Some(Self::Deepseek),
            "nvidia" | "nvidia-nim" | "nvidia_nim" | "nim" => Some(Self::NvidiaNim),
            "openai" | "open-ai" => Some(Self::Openai),
            "openrouter" | "open_router" => Some(Self::Openrouter),
            "novita" => Some(Self::Novita),
            "fireworks" | "fireworks-ai" => Some(Self::Fireworks),
            "sglang" | "sg-lang" => Some(Self::Sglang),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfigToml {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersToml {
    #[serde(default)]
    pub deepseek: ProviderConfigToml,
    #[serde(default)]
    pub nvidia_nim: ProviderConfigToml,
    #[serde(default)]
    pub openai: ProviderConfigToml,
    #[serde(default)]
    pub openrouter: ProviderConfigToml,
    #[serde(default)]
    pub novita: ProviderConfigToml,
    #[serde(default)]
    pub fireworks: ProviderConfigToml,
    #[serde(default)]
    pub sglang: ProviderConfigToml,
}

impl ProvidersToml {
    #[must_use]
    pub fn for_provider(&self, provider: ProviderKind) -> &ProviderConfigToml {
        match provider {
            ProviderKind::Deepseek => &self.deepseek,
            ProviderKind::NvidiaNim => &self.nvidia_nim,
            ProviderKind::Openai => &self.openai,
            ProviderKind::Openrouter => &self.openrouter,
            ProviderKind::Novita => &self.novita,
            ProviderKind::Fireworks => &self.fireworks,
            ProviderKind::Sglang => &self.sglang,
        }
    }

    pub fn for_provider_mut(&mut self, provider: ProviderKind) -> &mut ProviderConfigToml {
        match provider {
            ProviderKind::Deepseek => &mut self.deepseek,
            ProviderKind::NvidiaNim => &mut self.nvidia_nim,
            ProviderKind::Openai => &mut self.openai,
            ProviderKind::Openrouter => &mut self.openrouter,
            ProviderKind::Novita => &mut self.novita,
            ProviderKind::Fireworks => &mut self.fireworks,
            ProviderKind::Sglang => &mut self.sglang,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigToml {
    /// TUI-compatible DeepSeek API key. Kept at the root so both `deepseek`
    /// and `deepseek-tui` can share a single config file.
    pub api_key: Option<String>,
    /// TUI-compatible DeepSeek base URL.
    pub base_url: Option<String>,
    /// TUI-compatible default DeepSeek model.
    pub default_text_model: Option<String>,
    #[serde(default)]
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub auth_mode: Option<String>,
    pub chatgpt_access_token: Option<String>,
    pub device_code_session: Option<String>,
    pub output_mode: Option<String>,
    pub log_level: Option<String>,
    pub telemetry: Option<bool>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    #[serde(default)]
    pub providers: ProvidersToml,
    /// Per-domain network policy (#135). When absent, network tools fall back
    /// to a permissive default that mirrors pre-v0.7.0 behavior.
    #[serde(default)]
    pub network: Option<NetworkPolicyToml>,
    /// Community skill installer settings (#140). Mirrors
    /// [`SkillsToml`] from the TUI side; the dispatcher consults
    /// `registry_url` when running `deepseek skill install`.
    #[serde(default)]
    pub skills: Option<SkillsToml>,
    /// Workspace side-git snapshots (#137). The live TUI defaults this to
    /// enabled with 7-day retention when absent.
    #[serde(default)]
    pub snapshots: Option<SnapshotsToml>,
    /// Post-edit LSP diagnostics injection (#136). When absent, the engine
    /// applies the defaults documented in [`LspConfigToml`].
    #[serde(default)]
    pub lsp: Option<LspConfigToml>,
    #[serde(flatten)]
    pub extras: BTreeMap<String, toml::Value>,
}

/// On-disk schema for the `[skills]` table (#140). See `config.example.toml`
/// for documentation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsToml {
    /// Curated registry index URL. When unset, the TUI falls back to the
    /// bundled default (community-curated GitHub raw).
    #[serde(default)]
    pub registry_url: Option<String>,
    /// Per-skill maximum *uncompressed* size in bytes. When unset, the TUI
    /// uses 5 MiB.
    #[serde(default)]
    pub max_install_size_bytes: Option<u64>,
}

/// On-disk schema for the `[snapshots]` table (#137). See
/// `config.example.toml` for documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotsToml {
    #[serde(default = "default_snapshots_enabled")]
    pub enabled: bool,
    #[serde(default = "default_snapshot_max_age_days")]
    pub max_age_days: u64,
}

fn default_snapshots_enabled() -> bool {
    true
}

fn default_snapshot_max_age_days() -> u64 {
    7
}

impl Default for SnapshotsToml {
    fn default() -> Self {
        Self {
            enabled: default_snapshots_enabled(),
            max_age_days: default_snapshot_max_age_days(),
        }
    }
}

/// On-disk schema for the `[network]` table (#135). See `config.example.toml`
/// for documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicyToml {
    /// Decision for hosts that are not in `allow` or `deny`. One of
    /// `"allow" | "deny" | "prompt"`. Defaults to `"prompt"`.
    #[serde(default = "default_network_decision")]
    pub default: String,
    /// Hosts that are always allowed. Subdomain rules: a leading dot
    /// (`.example.com`) matches subdomains but not the apex.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Hosts that are always denied. Deny entries win over allow entries.
    #[serde(default)]
    pub deny: Vec<String>,
    /// Whether to record one audit-log line per outbound network call.
    #[serde(default = "default_network_audit")]
    pub audit: bool,
}

fn default_network_decision() -> String {
    "prompt".to_string()
}

fn default_network_audit() -> bool {
    true
}

impl Default for NetworkPolicyToml {
    fn default() -> Self {
        Self {
            default: default_network_decision(),
            allow: Vec::new(),
            deny: Vec::new(),
            audit: default_network_audit(),
        }
    }
}

/// On-disk schema for the `[lsp]` table (#136). See `config.example.toml`
/// for documentation. All fields are optional so the TUI runtime can fall
/// back to its own defaults when keys are absent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LspConfigToml {
    /// Master switch.
    pub enabled: Option<bool>,
    /// Maximum time to wait for diagnostics after an edit, in milliseconds.
    pub poll_after_edit_ms: Option<u64>,
    /// Cap on diagnostics surfaced per file.
    pub max_diagnostics_per_file: Option<usize>,
    /// When `true`, warnings (severity 2) are surfaced in addition to errors.
    pub include_warnings: Option<bool>,
    /// Optional override for the `language -> [cmd, ...args]` table.
    pub servers: Option<BTreeMap<String, Vec<String>>>,
}

impl ConfigToml {
    /// Merge project-level overrides from `$WORKSPACE/.deepseek/config.toml`.
    /// Only populated fields in `project` are applied; everything else
    /// keeps its global value. Provider-specific sub-tables are merged
    /// field-by-field so a project can set just `providers.deepseek.model`
    /// without needing to repeat `api_key` or `base_url`.
    pub fn merge_project_overrides(&mut self, project: ConfigToml) {
        // Check provider override condition before moving fields.
        let has_api_key = project.api_key.is_some();

        // Top-level scalar fields: apply when the project has a value.
        if has_api_key {
            self.api_key = project.api_key;
        }
        if project.base_url.is_some() {
            self.base_url = project.base_url;
        }
        if project.default_text_model.is_some() {
            self.default_text_model = project.default_text_model;
        }
        if project.model.is_some() {
            self.model = project.model;
        }
        if project.auth_mode.is_some() {
            self.auth_mode = project.auth_mode;
        }
        if project.output_mode.is_some() {
            self.output_mode = project.output_mode;
        }
        if project.telemetry.is_some() {
            self.telemetry = project.telemetry;
        }
        if project.approval_policy.is_some() {
            self.approval_policy = project.approval_policy;
        }
        if project.sandbox_mode.is_some() {
            self.sandbox_mode = project.sandbox_mode;
        }
        // Provider is only overridden if explicitly set (non-default).
        if project.provider != ProviderKind::Deepseek || has_api_key {
            self.provider = project.provider;
        }

        // Merge provider sub-tables field-by-field.
        merge_provider_config(&mut self.providers.deepseek, &project.providers.deepseek);
        merge_provider_config(
            &mut self.providers.nvidia_nim,
            &project.providers.nvidia_nim,
        );
        merge_provider_config(&mut self.providers.openai, &project.providers.openai);
        merge_provider_config(
            &mut self.providers.openrouter,
            &project.providers.openrouter,
        );
        merge_provider_config(&mut self.providers.novita, &project.providers.novita);
        merge_provider_config(&mut self.providers.fireworks, &project.providers.fireworks);
        merge_provider_config(&mut self.providers.sglang, &project.providers.sglang);

        if project.network.is_some() {
            self.network = project.network;
        }
        if project.skills.is_some() {
            self.skills = project.skills;
        }
        if project.snapshots.is_some() {
            self.snapshots = project.snapshots;
        }
        if project.lsp.is_some() {
            self.lsp = project.lsp;
        }
        for (k, v) in project.extras {
            self.extras.insert(k, v);
        }
    }

    #[must_use]
    pub fn get_value(&self, key: &str) -> Option<String> {
        match key {
            "provider" => Some(self.provider.as_str().to_string()),
            "api_key" => self.api_key.clone(),
            "base_url" => self.base_url.clone(),
            "default_text_model" => self.default_text_model.clone(),
            "model" => self.model.clone(),
            "auth.mode" => self.auth_mode.clone(),
            "auth.chatgpt_access_token" => self.chatgpt_access_token.clone(),
            "auth.device_code_session" => self.device_code_session.clone(),
            "output_mode" => self.output_mode.clone(),
            "log_level" => self.log_level.clone(),
            "telemetry" => self.telemetry.map(|v| v.to_string()),
            "approval_policy" => self.approval_policy.clone(),
            "sandbox_mode" => self.sandbox_mode.clone(),
            "providers.deepseek.api_key" => self.providers.deepseek.api_key.clone(),
            "providers.deepseek.base_url" => self.providers.deepseek.base_url.clone(),
            "providers.deepseek.model" => self.providers.deepseek.model.clone(),
            "providers.nvidia_nim.api_key" => self.providers.nvidia_nim.api_key.clone(),
            "providers.nvidia_nim.base_url" => self.providers.nvidia_nim.base_url.clone(),
            "providers.nvidia_nim.model" => self.providers.nvidia_nim.model.clone(),
            "providers.openai.api_key" => self.providers.openai.api_key.clone(),
            "providers.openai.base_url" => self.providers.openai.base_url.clone(),
            "providers.openai.model" => self.providers.openai.model.clone(),
            "providers.openrouter.api_key" => self.providers.openrouter.api_key.clone(),
            "providers.openrouter.base_url" => self.providers.openrouter.base_url.clone(),
            "providers.openrouter.model" => self.providers.openrouter.model.clone(),
            "providers.novita.api_key" => self.providers.novita.api_key.clone(),
            "providers.novita.base_url" => self.providers.novita.base_url.clone(),
            "providers.novita.model" => self.providers.novita.model.clone(),
            "providers.fireworks.api_key" => self.providers.fireworks.api_key.clone(),
            "providers.fireworks.base_url" => self.providers.fireworks.base_url.clone(),
            "providers.fireworks.model" => self.providers.fireworks.model.clone(),
            "providers.sglang.api_key" => self.providers.sglang.api_key.clone(),
            "providers.sglang.base_url" => self.providers.sglang.base_url.clone(),
            "providers.sglang.model" => self.providers.sglang.model.clone(),
            _ => self.extras.get(key).map(toml::Value::to_string),
        }
    }

    pub fn set_value(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "provider" => {
                self.provider = ProviderKind::parse(value)
                    .with_context(|| format!("unknown provider '{value}'"))?;
            }
            "api_key" => self.api_key = Some(value.to_string()),
            "base_url" => self.base_url = Some(value.to_string()),
            "default_text_model" => self.default_text_model = Some(value.to_string()),
            "model" => self.model = Some(value.to_string()),
            "auth.mode" => self.auth_mode = Some(value.to_string()),
            "auth.chatgpt_access_token" => self.chatgpt_access_token = Some(value.to_string()),
            "auth.device_code_session" => self.device_code_session = Some(value.to_string()),
            "output_mode" => self.output_mode = Some(value.to_string()),
            "log_level" => self.log_level = Some(value.to_string()),
            "telemetry" => {
                self.telemetry = Some(parse_bool(value)?);
            }
            "approval_policy" => self.approval_policy = Some(value.to_string()),
            "sandbox_mode" => self.sandbox_mode = Some(value.to_string()),
            "providers.deepseek.api_key" => {
                let value = value.to_string();
                self.providers.deepseek.api_key = Some(value.clone());
                self.api_key = Some(value);
            }
            "providers.deepseek.base_url" => {
                let value = value.to_string();
                self.providers.deepseek.base_url = Some(value.clone());
                self.base_url = Some(value);
            }
            "providers.deepseek.model" => {
                let value = value.to_string();
                self.providers.deepseek.model = Some(value.clone());
                self.default_text_model = Some(value);
            }
            "providers.openai.api_key" => self.providers.openai.api_key = Some(value.to_string()),
            "providers.openai.base_url" => self.providers.openai.base_url = Some(value.to_string()),
            "providers.openai.model" => self.providers.openai.model = Some(value.to_string()),
            "providers.nvidia_nim.api_key" => {
                self.providers.nvidia_nim.api_key = Some(value.to_string());
            }
            "providers.nvidia_nim.base_url" => {
                self.providers.nvidia_nim.base_url = Some(value.to_string());
            }
            "providers.nvidia_nim.model" => {
                self.providers.nvidia_nim.model = Some(value.to_string());
            }
            "providers.openrouter.api_key" => {
                self.providers.openrouter.api_key = Some(value.to_string());
            }
            "providers.openrouter.base_url" => {
                self.providers.openrouter.base_url = Some(value.to_string());
            }
            "providers.openrouter.model" => {
                self.providers.openrouter.model = Some(value.to_string());
            }
            "providers.novita.api_key" => {
                self.providers.novita.api_key = Some(value.to_string());
            }
            "providers.novita.base_url" => {
                self.providers.novita.base_url = Some(value.to_string());
            }
            "providers.novita.model" => {
                self.providers.novita.model = Some(value.to_string());
            }
            "providers.fireworks.api_key" => {
                self.providers.fireworks.api_key = Some(value.to_string());
            }
            "providers.fireworks.base_url" => {
                self.providers.fireworks.base_url = Some(value.to_string());
            }
            "providers.fireworks.model" => {
                self.providers.fireworks.model = Some(value.to_string());
            }
            "providers.sglang.api_key" => {
                self.providers.sglang.api_key = Some(value.to_string());
            }
            "providers.sglang.base_url" => {
                self.providers.sglang.base_url = Some(value.to_string());
            }
            "providers.sglang.model" => {
                self.providers.sglang.model = Some(value.to_string());
            }
            _ => {
                self.extras
                    .insert(key.to_string(), toml::Value::String(value.to_string()));
            }
        }
        Ok(())
    }

    pub fn unset_value(&mut self, key: &str) -> Result<()> {
        match key {
            "provider" => self.provider = ProviderKind::Deepseek,
            "api_key" => self.api_key = None,
            "base_url" => self.base_url = None,
            "default_text_model" => self.default_text_model = None,
            "model" => self.model = None,
            "auth.mode" => self.auth_mode = None,
            "auth.chatgpt_access_token" => self.chatgpt_access_token = None,
            "auth.device_code_session" => self.device_code_session = None,
            "output_mode" => self.output_mode = None,
            "log_level" => self.log_level = None,
            "telemetry" => self.telemetry = None,
            "approval_policy" => self.approval_policy = None,
            "sandbox_mode" => self.sandbox_mode = None,
            "providers.deepseek.api_key" => {
                self.providers.deepseek.api_key = None;
                self.api_key = None;
            }
            "providers.deepseek.base_url" => {
                self.providers.deepseek.base_url = None;
                self.base_url = None;
            }
            "providers.deepseek.model" => {
                self.providers.deepseek.model = None;
                self.default_text_model = None;
            }
            "providers.openai.api_key" => self.providers.openai.api_key = None,
            "providers.openai.base_url" => self.providers.openai.base_url = None,
            "providers.openai.model" => self.providers.openai.model = None,
            "providers.nvidia_nim.api_key" => self.providers.nvidia_nim.api_key = None,
            "providers.nvidia_nim.base_url" => self.providers.nvidia_nim.base_url = None,
            "providers.nvidia_nim.model" => self.providers.nvidia_nim.model = None,
            "providers.openrouter.api_key" => self.providers.openrouter.api_key = None,
            "providers.openrouter.base_url" => self.providers.openrouter.base_url = None,
            "providers.openrouter.model" => self.providers.openrouter.model = None,
            "providers.novita.api_key" => self.providers.novita.api_key = None,
            "providers.novita.base_url" => self.providers.novita.base_url = None,
            "providers.novita.model" => self.providers.novita.model = None,
            "providers.fireworks.api_key" => self.providers.fireworks.api_key = None,
            "providers.fireworks.base_url" => self.providers.fireworks.base_url = None,
            "providers.fireworks.model" => self.providers.fireworks.model = None,
            "providers.sglang.api_key" => self.providers.sglang.api_key = None,
            "providers.sglang.base_url" => self.providers.sglang.base_url = None,
            "providers.sglang.model" => self.providers.sglang.model = None,
            _ => {
                self.extras.remove(key);
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn list_values(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        out.insert("provider".to_string(), self.provider.as_str().to_string());

        if let Some(v) = self.api_key.as_ref() {
            out.insert("api_key".to_string(), redact_secret(v));
        }
        if let Some(v) = self.base_url.as_ref() {
            out.insert("base_url".to_string(), v.clone());
        }
        if let Some(v) = self.default_text_model.as_ref() {
            out.insert("default_text_model".to_string(), v.clone());
        }
        if let Some(v) = self.model.as_ref() {
            out.insert("model".to_string(), v.clone());
        }
        if let Some(v) = self.auth_mode.as_ref() {
            out.insert("auth.mode".to_string(), v.clone());
        }
        if let Some(v) = self.chatgpt_access_token.as_ref() {
            out.insert("auth.chatgpt_access_token".to_string(), redact_secret(v));
        }
        if let Some(v) = self.device_code_session.as_ref() {
            out.insert("auth.device_code_session".to_string(), redact_secret(v));
        }
        if let Some(v) = self.output_mode.as_ref() {
            out.insert("output_mode".to_string(), v.clone());
        }
        if let Some(v) = self.log_level.as_ref() {
            out.insert("log_level".to_string(), v.clone());
        }
        if let Some(v) = self.telemetry {
            out.insert("telemetry".to_string(), v.to_string());
        }
        if let Some(v) = self.approval_policy.as_ref() {
            out.insert("approval_policy".to_string(), v.clone());
        }
        if let Some(v) = self.sandbox_mode.as_ref() {
            out.insert("sandbox_mode".to_string(), v.clone());
        }
        if let Some(v) = self.providers.deepseek.api_key.as_ref() {
            out.insert("providers.deepseek.api_key".to_string(), redact_secret(v));
        }
        if let Some(v) = self.providers.deepseek.base_url.as_ref() {
            out.insert("providers.deepseek.base_url".to_string(), v.clone());
        }
        if let Some(v) = self.providers.deepseek.model.as_ref() {
            out.insert("providers.deepseek.model".to_string(), v.clone());
        }
        if let Some(v) = self.providers.openai.api_key.as_ref() {
            out.insert("providers.openai.api_key".to_string(), redact_secret(v));
        }
        if let Some(v) = self.providers.openai.base_url.as_ref() {
            out.insert("providers.openai.base_url".to_string(), v.clone());
        }
        if let Some(v) = self.providers.openai.model.as_ref() {
            out.insert("providers.openai.model".to_string(), v.clone());
        }
        if let Some(v) = self.providers.nvidia_nim.api_key.as_ref() {
            out.insert("providers.nvidia_nim.api_key".to_string(), redact_secret(v));
        }
        if let Some(v) = self.providers.nvidia_nim.base_url.as_ref() {
            out.insert("providers.nvidia_nim.base_url".to_string(), v.clone());
        }
        if let Some(v) = self.providers.nvidia_nim.model.as_ref() {
            out.insert("providers.nvidia_nim.model".to_string(), v.clone());
        }
        if let Some(v) = self.providers.openrouter.api_key.as_ref() {
            out.insert("providers.openrouter.api_key".to_string(), redact_secret(v));
        }
        if let Some(v) = self.providers.openrouter.base_url.as_ref() {
            out.insert("providers.openrouter.base_url".to_string(), v.clone());
        }
        if let Some(v) = self.providers.openrouter.model.as_ref() {
            out.insert("providers.openrouter.model".to_string(), v.clone());
        }
        if let Some(v) = self.providers.novita.api_key.as_ref() {
            out.insert("providers.novita.api_key".to_string(), redact_secret(v));
        }
        if let Some(v) = self.providers.novita.base_url.as_ref() {
            out.insert("providers.novita.base_url".to_string(), v.clone());
        }
        if let Some(v) = self.providers.novita.model.as_ref() {
            out.insert("providers.novita.model".to_string(), v.clone());
        }
        if let Some(v) = self.providers.fireworks.api_key.as_ref() {
            out.insert("providers.fireworks.api_key".to_string(), redact_secret(v));
        }
        if let Some(v) = self.providers.fireworks.base_url.as_ref() {
            out.insert("providers.fireworks.base_url".to_string(), v.clone());
        }
        if let Some(v) = self.providers.fireworks.model.as_ref() {
            out.insert("providers.fireworks.model".to_string(), v.clone());
        }
        if let Some(v) = self.providers.sglang.api_key.as_ref() {
            out.insert("providers.sglang.api_key".to_string(), redact_secret(v));
        }
        if let Some(v) = self.providers.sglang.base_url.as_ref() {
            out.insert("providers.sglang.base_url".to_string(), v.clone());
        }
        if let Some(v) = self.providers.sglang.model.as_ref() {
            out.insert("providers.sglang.model".to_string(), v.clone());
        }

        for (k, v) in &self.extras {
            out.insert(k.clone(), v.to_string());
        }
        out
    }

    /// Resolve runtime options without touching platform credential stores.
    ///
    /// v0.8.8 keeps the default auth path deliberately boring:
    /// CLI flag → config file → environment. Explicit keyring migration
    /// remains available through auth commands, but normal startup and
    /// diagnostics must not trigger platform credential prompts.
    #[must_use]
    pub fn resolve_runtime_options(&self, cli: &CliRuntimeOverrides) -> ResolvedRuntimeOptions {
        let no_keyring = Secrets::new(std::sync::Arc::new(
            deepseek_secrets::InMemoryKeyringStore::new(),
        ));
        self.resolve_runtime_options_with_secrets(cli, &no_keyring)
    }

    /// Resolve runtime options using an explicit secrets façade.
    ///
    /// API-key precedence is **CLI flag → config-file → environment**.
    /// If a caller explicitly injects a secrets façade with a populated
    /// credential store, that store is used only when config/env are empty.
    #[must_use]
    pub fn resolve_runtime_options_with_secrets(
        &self,
        cli: &CliRuntimeOverrides,
        secrets: &Secrets,
    ) -> ResolvedRuntimeOptions {
        let env = EnvRuntimeOverrides::load();
        let provider = cli.provider.or(env.provider).unwrap_or(self.provider);

        let provider_cfg = self.providers.for_provider(provider);
        let root_deepseek_api_key = (provider == ProviderKind::Deepseek)
            .then(|| self.api_key.clone())
            .flatten();
        let root_deepseek_base_url = (provider == ProviderKind::Deepseek)
            .then(|| self.base_url.clone())
            .flatten();
        let root_deepseek_model = (provider == ProviderKind::Deepseek)
            .then(|| self.default_text_model.clone())
            .flatten();
        // CLI flag wins outright. Otherwise: config-file → injected secrets/env.
        // This makes `deepseek auth set` a reliable fix even when the user's
        // shell still exports an old key. The default caller injects an empty
        // in-memory store, so this path does not touch platform credential
        // stores during ordinary startup.
        let from_file = provider_cfg.api_key.clone().or(root_deepseek_api_key);
        let api_key = cli
            .api_key
            .clone()
            .or_else(|| from_file.clone())
            .or_else(|| secrets.resolve(provider.as_str()));

        let base_url = cli
            .base_url
            .clone()
            .or_else(|| env.base_url_for(provider))
            .or_else(|| provider_cfg.base_url.clone())
            .or(root_deepseek_base_url)
            .unwrap_or_else(|| match provider {
                ProviderKind::Deepseek => DEFAULT_DEEPSEEK_BASE_URL.to_string(),
                ProviderKind::NvidiaNim => DEFAULT_NVIDIA_NIM_BASE_URL.to_string(),
                ProviderKind::Openai => DEFAULT_OPENAI_BASE_URL.to_string(),
                ProviderKind::Openrouter => DEFAULT_OPENROUTER_BASE_URL.to_string(),
                ProviderKind::Novita => DEFAULT_NOVITA_BASE_URL.to_string(),
                ProviderKind::Fireworks => DEFAULT_FIREWORKS_BASE_URL.to_string(),
                ProviderKind::Sglang => DEFAULT_SGLANG_BASE_URL.to_string(),
            });

        let model = cli
            .model
            .clone()
            .or_else(|| env.model.clone())
            .or_else(|| provider_cfg.model.clone())
            .or(root_deepseek_model)
            .or_else(|| self.model.clone())
            .unwrap_or_else(|| match provider {
                ProviderKind::Deepseek => DEFAULT_DEEPSEEK_MODEL.to_string(),
                ProviderKind::NvidiaNim => DEFAULT_NVIDIA_NIM_MODEL.to_string(),
                ProviderKind::Openai => DEFAULT_OPENAI_MODEL.to_string(),
                ProviderKind::Openrouter => DEFAULT_OPENROUTER_MODEL.to_string(),
                ProviderKind::Novita => DEFAULT_NOVITA_MODEL.to_string(),
                ProviderKind::Fireworks => DEFAULT_FIREWORKS_MODEL.to_string(),
                ProviderKind::Sglang => DEFAULT_SGLANG_MODEL.to_string(),
            });
        let model = normalize_model_for_provider(provider, &model);

        let output_mode = cli
            .output_mode
            .clone()
            .or_else(|| env.output_mode.clone())
            .or_else(|| self.output_mode.clone());
        let auth_mode = cli
            .auth_mode
            .clone()
            .or_else(|| env.auth_mode.clone())
            .or_else(|| self.auth_mode.clone());
        let log_level = cli
            .log_level
            .clone()
            .or_else(|| env.log_level.clone())
            .or_else(|| self.log_level.clone());
        let telemetry = cli
            .telemetry
            .or(env.telemetry)
            .or(self.telemetry)
            .unwrap_or(false);
        let approval_policy = cli
            .approval_policy
            .clone()
            .or_else(|| env.approval_policy.clone())
            .or_else(|| self.approval_policy.clone());
        let sandbox_mode = cli
            .sandbox_mode
            .clone()
            .or_else(|| env.sandbox_mode.clone())
            .or_else(|| self.sandbox_mode.clone());

        ResolvedRuntimeOptions {
            provider,
            model,
            api_key,
            base_url,
            auth_mode,
            output_mode,
            log_level,
            telemetry,
            approval_policy,
            sandbox_mode,
        }
    }
}

fn merge_provider_config(target: &mut ProviderConfigToml, source: &ProviderConfigToml) {
    if source.api_key.is_some() {
        target.api_key = source.api_key.clone();
    }
    if source.base_url.is_some() {
        target.base_url = source.base_url.clone();
    }
    if source.model.is_some() {
        target.model = source.model.clone();
    }
}

/// Load a project-level config from `$WORKSPACE/.deepseek/config.toml`.
/// Returns `None` if the file doesn't exist or can't be parsed.
pub fn load_project_config(workspace: &Path) -> Option<ConfigToml> {
    let path = workspace.join(".deepseek").join(CONFIG_FILE_NAME);
    if !path.exists() {
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    toml::from_str(&raw).ok()
}

fn normalize_model_for_provider(provider: ProviderKind, model: &str) -> String {
    let normalized = model.trim().to_ascii_lowercase();
    match (provider, normalized.as_str()) {
        (ProviderKind::NvidiaNim, "deepseek-v4-pro" | "deepseek-v4pro") => {
            DEFAULT_NVIDIA_NIM_MODEL.to_string()
        }
        (
            ProviderKind::NvidiaNim,
            "deepseek-v4-flash" | "deepseek-v4flash" | "deepseek-chat" | "deepseek-reasoner"
            | "deepseek-r1" | "deepseek-v3" | "deepseek-v3.2",
        ) => DEFAULT_NVIDIA_NIM_FLASH_MODEL.to_string(),
        (ProviderKind::Openrouter, "deepseek-v4-pro" | "deepseek-v4pro") => {
            DEFAULT_OPENROUTER_MODEL.to_string()
        }
        (
            ProviderKind::Openrouter,
            "deepseek-v4-flash" | "deepseek-v4flash" | "deepseek-chat" | "deepseek-reasoner"
            | "deepseek-r1" | "deepseek-v3" | "deepseek-v3.2",
        ) => DEFAULT_OPENROUTER_FLASH_MODEL.to_string(),
        (ProviderKind::Novita, "deepseek-v4-pro" | "deepseek-v4pro") => {
            DEFAULT_NOVITA_MODEL.to_string()
        }
        (
            ProviderKind::Novita,
            "deepseek-v4-flash" | "deepseek-v4flash" | "deepseek-chat" | "deepseek-reasoner"
            | "deepseek-r1" | "deepseek-v3" | "deepseek-v3.2",
        ) => DEFAULT_NOVITA_FLASH_MODEL.to_string(),
        (ProviderKind::Fireworks, "deepseek-v4-pro" | "deepseek-v4pro") => {
            DEFAULT_FIREWORKS_MODEL.to_string()
        }
        (ProviderKind::Sglang, "deepseek-v4-pro" | "deepseek-v4pro") => {
            DEFAULT_SGLANG_MODEL.to_string()
        }
        (
            ProviderKind::Sglang,
            "deepseek-v4-flash" | "deepseek-v4flash" | "deepseek-chat" | "deepseek-reasoner"
            | "deepseek-r1" | "deepseek-v3" | "deepseek-v3.2",
        ) => DEFAULT_SGLANG_FLASH_MODEL.to_string(),
        _ => model.to_string(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct CliRuntimeOverrides {
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub auth_mode: Option<String>,
    pub output_mode: Option<String>,
    pub log_level: Option<String>,
    pub telemetry: Option<bool>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedRuntimeOptions {
    pub provider: ProviderKind,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub auth_mode: Option<String>,
    pub output_mode: Option<String>,
    pub log_level: Option<String>,
    pub telemetry: bool,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
    pub config: ConfigToml,
}

impl ConfigStore {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let path = resolve_config_path(path)?;
        if !path.exists() {
            return Ok(Self {
                path,
                config: ConfigToml::default(),
            });
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let parsed: ConfigToml = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;

        Ok(Self {
            path,
            config: parsed,
        })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        let body = toml::to_string_pretty(&self.config).context("failed to serialize config")?;
        fs::write(&self.path, body)
            .with_context(|| format!("failed to write config at {}", self.path.display()))?;
        Ok(())
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Process-wide default [`Secrets`] façade. The first caller wins; the
/// lock is exposed so test or CLI code can install an explicit
/// backend (e.g. an [`deepseek_secrets::InMemoryKeyringStore`]) before
/// any resolver runs.
pub fn default_secrets() -> &'static Secrets {
    static SECRETS: OnceLock<Secrets> = OnceLock::new();
    SECRETS.get_or_init(|| {
        // Tests should never poke real platform credential stores. Cargo sets the
        // `RUST_TEST_*` family of env vars (and `CARGO_PKG_NAME` is
        // always populated), but the `cfg(test)` flag is the canonical
        // signal here. See `install_test_secrets` for explicit installs.
        #[cfg(test)]
        {
            Secrets::new(std::sync::Arc::new(
                deepseek_secrets::InMemoryKeyringStore::new(),
            ))
        }
        #[cfg(not(test))]
        {
            Secrets::auto_detect()
        }
    })
}

pub fn resolve_config_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Ok(path) = std::env::var("DEEPSEEK_CONFIG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    default_config_path()
}

pub fn default_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("failed to resolve home directory for config path")?;
    Ok(home.join(".deepseek").join(CONFIG_FILE_NAME))
}

fn parse_bool(raw: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enabled" => Ok(true),
        "0" | "false" | "no" | "off" | "disabled" => Ok(false),
        _ => bail!("invalid boolean '{raw}'"),
    }
}

fn redact_secret(secret: &str) -> String {
    if secret.len() <= 8 {
        return "********".to_string();
    }
    format!("{}***{}", &secret[..4], &secret[secret.len() - 4..])
}

#[derive(Debug, Clone, Default)]
struct EnvRuntimeOverrides {
    provider: Option<ProviderKind>,
    model: Option<String>,
    output_mode: Option<String>,
    auth_mode: Option<String>,
    log_level: Option<String>,
    telemetry: Option<bool>,
    approval_policy: Option<String>,
    sandbox_mode: Option<String>,
    deepseek_base_url: Option<String>,
    nvidia_base_url: Option<String>,
    openai_base_url: Option<String>,
    openrouter_base_url: Option<String>,
    novita_base_url: Option<String>,
    fireworks_base_url: Option<String>,
    sglang_base_url: Option<String>,
}

impl EnvRuntimeOverrides {
    fn load() -> Self {
        Self {
            provider: std::env::var("DEEPSEEK_PROVIDER")
                .ok()
                .and_then(|v| ProviderKind::parse(&v)),
            model: std::env::var("DEEPSEEK_MODEL").ok(),
            output_mode: std::env::var("DEEPSEEK_OUTPUT_MODE").ok(),
            auth_mode: std::env::var("DEEPSEEK_AUTH_MODE").ok(),
            log_level: std::env::var("DEEPSEEK_LOG_LEVEL").ok(),
            telemetry: std::env::var("DEEPSEEK_TELEMETRY")
                .ok()
                .and_then(|v| parse_bool(&v).ok()),
            approval_policy: std::env::var("DEEPSEEK_APPROVAL_POLICY").ok(),
            sandbox_mode: std::env::var("DEEPSEEK_SANDBOX_MODE").ok(),
            deepseek_base_url: std::env::var("DEEPSEEK_BASE_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            nvidia_base_url: std::env::var("NVIDIA_NIM_BASE_URL")
                .or_else(|_| std::env::var("NIM_BASE_URL"))
                .or_else(|_| std::env::var("NVIDIA_BASE_URL"))
                .ok()
                .filter(|v| !v.trim().is_empty()),
            openai_base_url: std::env::var("OPENAI_BASE_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            openrouter_base_url: std::env::var("OPENROUTER_BASE_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            novita_base_url: std::env::var("NOVITA_BASE_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            fireworks_base_url: std::env::var("FIREWORKS_BASE_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            sglang_base_url: std::env::var("SGLANG_BASE_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
        }
    }

    fn base_url_for(&self, provider: ProviderKind) -> Option<String> {
        // Defaults belong in the resolver's final fallback so config-file
        // values (`providers.<name>.base_url`) still win when env is unset.
        match provider {
            ProviderKind::Deepseek => self.deepseek_base_url.clone(),
            ProviderKind::NvidiaNim => self.nvidia_base_url.clone(),
            ProviderKind::Openai => self.openai_base_url.clone(),
            ProviderKind::Openrouter => self.openrouter_base_url.clone(),
            ProviderKind::Novita => self.novita_base_url.clone(),
            ProviderKind::Fireworks => self.fireworks_base_url.clone(),
            ProviderKind::Sglang => self.sglang_base_url.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    struct EnvGuard {
        deepseek_api_key: Option<OsString>,
        deepseek_base_url: Option<OsString>,
        deepseek_model: Option<OsString>,
        deepseek_provider: Option<OsString>,
        nvidia_api_key: Option<OsString>,
        nvidia_nim_api_key: Option<OsString>,
        nim_base_url: Option<OsString>,
        nvidia_base_url: Option<OsString>,
        nvidia_nim_base_url: Option<OsString>,
        openrouter_api_key: Option<OsString>,
        openrouter_base_url: Option<OsString>,
        novita_api_key: Option<OsString>,
        novita_base_url: Option<OsString>,
        fireworks_api_key: Option<OsString>,
        fireworks_base_url: Option<OsString>,
        sglang_api_key: Option<OsString>,
        sglang_base_url: Option<OsString>,
    }

    impl EnvGuard {
        fn without_deepseek_runtime_overrides() -> Self {
            let guard = Self {
                deepseek_api_key: env::var_os("DEEPSEEK_API_KEY"),
                deepseek_base_url: env::var_os("DEEPSEEK_BASE_URL"),
                deepseek_model: env::var_os("DEEPSEEK_MODEL"),
                deepseek_provider: env::var_os("DEEPSEEK_PROVIDER"),
                nvidia_api_key: env::var_os("NVIDIA_API_KEY"),
                nvidia_nim_api_key: env::var_os("NVIDIA_NIM_API_KEY"),
                nim_base_url: env::var_os("NIM_BASE_URL"),
                nvidia_base_url: env::var_os("NVIDIA_BASE_URL"),
                nvidia_nim_base_url: env::var_os("NVIDIA_NIM_BASE_URL"),
                openrouter_api_key: env::var_os("OPENROUTER_API_KEY"),
                openrouter_base_url: env::var_os("OPENROUTER_BASE_URL"),
                novita_api_key: env::var_os("NOVITA_API_KEY"),
                novita_base_url: env::var_os("NOVITA_BASE_URL"),
                fireworks_api_key: env::var_os("FIREWORKS_API_KEY"),
                fireworks_base_url: env::var_os("FIREWORKS_BASE_URL"),
                sglang_api_key: env::var_os("SGLANG_API_KEY"),
                sglang_base_url: env::var_os("SGLANG_BASE_URL"),
            };
            // Safety: test-only environment mutation guarded by a module mutex.
            unsafe {
                env::remove_var("DEEPSEEK_API_KEY");
                env::remove_var("DEEPSEEK_BASE_URL");
                env::remove_var("DEEPSEEK_MODEL");
                env::remove_var("DEEPSEEK_PROVIDER");
                env::remove_var("NVIDIA_API_KEY");
                env::remove_var("NVIDIA_NIM_API_KEY");
                env::remove_var("NIM_BASE_URL");
                env::remove_var("NVIDIA_BASE_URL");
                env::remove_var("NVIDIA_NIM_BASE_URL");
                env::remove_var("OPENROUTER_API_KEY");
                env::remove_var("OPENROUTER_BASE_URL");
                env::remove_var("NOVITA_API_KEY");
                env::remove_var("NOVITA_BASE_URL");
                env::remove_var("FIREWORKS_API_KEY");
                env::remove_var("FIREWORKS_BASE_URL");
                env::remove_var("SGLANG_API_KEY");
                env::remove_var("SGLANG_BASE_URL");
            }
            guard
        }

        unsafe fn restore_var(key: &str, value: Option<OsString>) {
            if let Some(value) = value {
                unsafe { env::set_var(key, value) };
            } else {
                unsafe { env::remove_var(key) };
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // Safety: test-only environment mutation guarded by a module mutex.
            unsafe {
                Self::restore_var("DEEPSEEK_API_KEY", self.deepseek_api_key.take());
                Self::restore_var("DEEPSEEK_BASE_URL", self.deepseek_base_url.take());
                Self::restore_var("DEEPSEEK_MODEL", self.deepseek_model.take());
                Self::restore_var("DEEPSEEK_PROVIDER", self.deepseek_provider.take());
                Self::restore_var("NVIDIA_API_KEY", self.nvidia_api_key.take());
                Self::restore_var("NVIDIA_NIM_API_KEY", self.nvidia_nim_api_key.take());
                Self::restore_var("NIM_BASE_URL", self.nim_base_url.take());
                Self::restore_var("NVIDIA_BASE_URL", self.nvidia_base_url.take());
                Self::restore_var("NVIDIA_NIM_BASE_URL", self.nvidia_nim_base_url.take());
                Self::restore_var("OPENROUTER_API_KEY", self.openrouter_api_key.take());
                Self::restore_var("OPENROUTER_BASE_URL", self.openrouter_base_url.take());
                Self::restore_var("NOVITA_API_KEY", self.novita_api_key.take());
                Self::restore_var("NOVITA_BASE_URL", self.novita_base_url.take());
                Self::restore_var("FIREWORKS_API_KEY", self.fireworks_api_key.take());
                Self::restore_var("FIREWORKS_BASE_URL", self.fireworks_base_url.take());
                Self::restore_var("SGLANG_API_KEY", self.sglang_api_key.take());
                Self::restore_var("SGLANG_BASE_URL", self.sglang_base_url.take());
            }
        }
    }

    #[test]
    fn root_deepseek_fields_are_runtime_fallbacks() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let config = ConfigToml {
            api_key: Some("root-key".to_string()),
            base_url: Some("https://api.deepseek.com".to_string()),
            default_text_model: Some("deepseek-v4-pro".to_string()),
            ..ConfigToml::default()
        };

        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::Deepseek);
        assert_eq!(resolved.api_key.as_deref(), Some("root-key"));
        assert_eq!(resolved.base_url, "https://api.deepseek.com");
        assert_eq!(resolved.model, "deepseek-v4-pro");
    }

    #[test]
    fn provider_specific_deepseek_fields_override_tui_compat_fields() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let mut config = ConfigToml {
            api_key: Some("root-key".to_string()),
            base_url: Some("https://api.deepseek.com".to_string()),
            default_text_model: Some("deepseek-v4-pro".to_string()),
            ..ConfigToml::default()
        };
        config.providers.deepseek.api_key = Some("provider-key".to_string());
        config.providers.deepseek.base_url = Some("https://api.deepseeki.com".to_string());
        config.providers.deepseek.model = Some("deepseek-v4-flash".to_string());

        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.api_key.as_deref(), Some("provider-key"));
        assert_eq!(resolved.base_url, "https://api.deepseeki.com");
        assert_eq!(resolved.model, "deepseek-v4-flash");
    }

    #[test]
    fn nvidia_nim_provider_defaults_to_catalog_endpoint_and_model() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let config = ConfigToml {
            provider: ProviderKind::NvidiaNim,
            ..ConfigToml::default()
        };

        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::NvidiaNim);
        assert_eq!(resolved.base_url, DEFAULT_NVIDIA_NIM_BASE_URL);
        assert_eq!(resolved.model, DEFAULT_NVIDIA_NIM_MODEL);
    }

    #[test]
    fn nvidia_nim_provider_uses_provider_specific_credentials() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let mut config = ConfigToml {
            provider: ProviderKind::NvidiaNim,
            ..ConfigToml::default()
        };
        config.providers.nvidia_nim.api_key = Some("nim-key".to_string());
        config.providers.nvidia_nim.base_url = Some("https://nim.example/v1".to_string());
        config.providers.nvidia_nim.model = Some("deepseek-ai/deepseek-v4-pro".to_string());

        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::NvidiaNim);
        assert_eq!(resolved.api_key.as_deref(), Some("nim-key"));
        assert_eq!(resolved.base_url, "https://nim.example/v1");
        assert_eq!(resolved.model, "deepseek-ai/deepseek-v4-pro");
    }

    #[test]
    fn nvidia_nim_provider_normalizes_flash_aliases() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let cli = CliRuntimeOverrides {
            provider: Some(ProviderKind::NvidiaNim),
            model: Some("deepseek-v4-flash".to_string()),
            ..CliRuntimeOverrides::default()
        };

        let resolved = ConfigToml::default().resolve_runtime_options(&cli);

        assert_eq!(resolved.provider, ProviderKind::NvidiaNim);
        assert_eq!(resolved.model, DEFAULT_NVIDIA_NIM_FLASH_MODEL);
    }

    #[test]
    fn nvidia_nim_provider_uses_nvidia_env_credentials() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        // Safety: test-only environment mutation guarded by a module mutex.
        unsafe {
            env::set_var("DEEPSEEK_PROVIDER", "nvidia-nim");
            env::set_var("NVIDIA_API_KEY", "nim-env-key");
            env::set_var("NVIDIA_NIM_BASE_URL", "https://nim-env.example/v1");
        }

        let config = ConfigToml::default();
        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::NvidiaNim);
        assert_eq!(resolved.api_key.as_deref(), Some("nim-env-key"));
        assert_eq!(resolved.base_url, "https://nim-env.example/v1");
        assert_eq!(resolved.model, DEFAULT_NVIDIA_NIM_MODEL);
    }

    #[test]
    fn nvidia_nim_provider_accepts_short_nim_base_url_alias() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        // Safety: test-only environment mutation guarded by a module mutex.
        unsafe {
            env::set_var("DEEPSEEK_PROVIDER", "nvidia-nim");
            env::set_var("NVIDIA_API_KEY", "nim-env-key");
            env::set_var("NIM_BASE_URL", "https://short-nim.example/v1");
        }

        let config = ConfigToml::default();
        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::NvidiaNim);
        assert_eq!(resolved.base_url, "https://short-nim.example/v1");
    }

    #[test]
    fn nvidia_nim_provider_can_fallback_to_deepseek_api_key_env() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        // Safety: test-only environment mutation guarded by a module mutex.
        unsafe {
            env::set_var("DEEPSEEK_PROVIDER", "nvidia-nim");
            env::set_var("DEEPSEEK_API_KEY", "deepseek-compat-key");
        }

        let config = ConfigToml::default();
        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::NvidiaNim);
        assert_eq!(resolved.api_key.as_deref(), Some("deepseek-compat-key"));
    }

    #[test]
    fn list_values_redacts_root_api_key() {
        let config = ConfigToml {
            api_key: Some("sk-deepseek-secret".to_string()),
            ..ConfigToml::default()
        };

        let values = config.list_values();

        assert_eq!(
            values.get("api_key").map(String::as_str),
            Some("sk-d***cret")
        );
    }

    #[test]
    fn provider_kind_parses_openrouter_and_novita_aliases() {
        assert_eq!(
            ProviderKind::parse("openrouter"),
            Some(ProviderKind::Openrouter)
        );
        assert_eq!(
            ProviderKind::parse("OPEN_ROUTER"),
            Some(ProviderKind::Openrouter)
        );
        assert_eq!(ProviderKind::parse("novita"), Some(ProviderKind::Novita));
        assert_eq!(ProviderKind::parse("Novita"), Some(ProviderKind::Novita));
        assert_eq!(
            ProviderKind::parse("fireworks-ai"),
            Some(ProviderKind::Fireworks)
        );
        assert_eq!(ProviderKind::parse("sg-lang"), Some(ProviderKind::Sglang));
    }

    #[test]
    fn openrouter_provider_defaults_to_canonical_endpoint_and_model() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let config = ConfigToml {
            provider: ProviderKind::Openrouter,
            ..ConfigToml::default()
        };

        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::Openrouter);
        assert_eq!(resolved.base_url, DEFAULT_OPENROUTER_BASE_URL);
        assert_eq!(resolved.model, DEFAULT_OPENROUTER_MODEL);
    }

    #[test]
    fn novita_provider_defaults_to_canonical_endpoint_and_model() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let config = ConfigToml {
            provider: ProviderKind::Novita,
            ..ConfigToml::default()
        };

        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::Novita);
        assert_eq!(resolved.base_url, DEFAULT_NOVITA_BASE_URL);
        assert_eq!(resolved.model, DEFAULT_NOVITA_MODEL);
    }

    #[test]
    fn fireworks_provider_defaults_to_canonical_endpoint_and_model() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let config = ConfigToml {
            provider: ProviderKind::Fireworks,
            ..ConfigToml::default()
        };

        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::Fireworks);
        assert_eq!(resolved.base_url, DEFAULT_FIREWORKS_BASE_URL);
        assert_eq!(resolved.model, DEFAULT_FIREWORKS_MODEL);
    }

    #[test]
    fn sglang_provider_defaults_to_local_endpoint_and_model() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let config = ConfigToml {
            provider: ProviderKind::Sglang,
            ..ConfigToml::default()
        };

        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::Sglang);
        assert_eq!(resolved.base_url, DEFAULT_SGLANG_BASE_URL);
        assert_eq!(resolved.model, DEFAULT_SGLANG_MODEL);
    }

    #[test]
    fn openrouter_env_api_key_falls_back_when_config_missing() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        // Safety: test-only environment mutation guarded by a module mutex.
        unsafe {
            env::set_var("DEEPSEEK_PROVIDER", "openrouter");
            env::set_var("OPENROUTER_API_KEY", "or-env-key");
        }

        let resolved =
            ConfigToml::default().resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::Openrouter);
        assert_eq!(resolved.api_key.as_deref(), Some("or-env-key"));
        assert_eq!(resolved.base_url, DEFAULT_OPENROUTER_BASE_URL);
    }

    #[test]
    fn novita_env_api_key_falls_back_when_config_missing() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        // Safety: test-only environment mutation guarded by a module mutex.
        unsafe {
            env::set_var("DEEPSEEK_PROVIDER", "novita");
            env::set_var("NOVITA_API_KEY", "novita-env-key");
        }

        let resolved =
            ConfigToml::default().resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::Novita);
        assert_eq!(resolved.api_key.as_deref(), Some("novita-env-key"));
        assert_eq!(resolved.base_url, DEFAULT_NOVITA_BASE_URL);
    }

    #[test]
    fn fireworks_env_api_key_falls_back_when_config_missing() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        // Safety: test-only environment mutation guarded by a module mutex.
        unsafe {
            env::set_var("DEEPSEEK_PROVIDER", "fireworks");
            env::set_var("FIREWORKS_API_KEY", "fw-env-key");
        }

        let resolved =
            ConfigToml::default().resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.provider, ProviderKind::Fireworks);
        assert_eq!(resolved.api_key.as_deref(), Some("fw-env-key"));
        assert_eq!(resolved.base_url, DEFAULT_FIREWORKS_BASE_URL);
    }

    #[test]
    fn openrouter_provider_normalizes_flash_aliases() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let cli = CliRuntimeOverrides {
            provider: Some(ProviderKind::Openrouter),
            model: Some("deepseek-v4-flash".to_string()),
            ..CliRuntimeOverrides::default()
        };

        let resolved = ConfigToml::default().resolve_runtime_options(&cli);

        assert_eq!(resolved.provider, ProviderKind::Openrouter);
        assert_eq!(resolved.model, DEFAULT_OPENROUTER_FLASH_MODEL);
    }

    #[test]
    fn novita_provider_normalizes_flash_aliases() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let cli = CliRuntimeOverrides {
            provider: Some(ProviderKind::Novita),
            model: Some("deepseek-v4-flash".to_string()),
            ..CliRuntimeOverrides::default()
        };

        let resolved = ConfigToml::default().resolve_runtime_options(&cli);

        assert_eq!(resolved.provider, ProviderKind::Novita);
        assert_eq!(resolved.model, DEFAULT_NOVITA_FLASH_MODEL);
    }

    #[test]
    fn sglang_provider_normalizes_flash_aliases() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let cli = CliRuntimeOverrides {
            provider: Some(ProviderKind::Sglang),
            model: Some("deepseek-v4-flash".to_string()),
            ..CliRuntimeOverrides::default()
        };

        let resolved = ConfigToml::default().resolve_runtime_options(&cli);

        assert_eq!(resolved.provider, ProviderKind::Sglang);
        assert_eq!(resolved.model, DEFAULT_SGLANG_FLASH_MODEL);
    }

    #[test]
    fn openrouter_provider_specific_config_overrides_env() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        let mut config = ConfigToml {
            provider: ProviderKind::Openrouter,
            ..ConfigToml::default()
        };
        config.providers.openrouter.api_key = Some("file-key".to_string());
        config.providers.openrouter.base_url = Some("https://or-mirror.example/v1".to_string());

        let resolved = config.resolve_runtime_options(&CliRuntimeOverrides::default());

        assert_eq!(resolved.api_key.as_deref(), Some("file-key"));
        assert_eq!(resolved.base_url, "https://or-mirror.example/v1");
    }

    #[test]
    fn config_file_resolves_above_env_and_keyring() {
        use deepseek_secrets::KeyringStore;
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        // Safety: env mutation guarded by env_lock().
        unsafe { std::env::set_var("DEEPSEEK_API_KEY", "env-key") };

        let store = std::sync::Arc::new(deepseek_secrets::InMemoryKeyringStore::new());
        store.set("deepseek", "ring-key").unwrap();
        let secrets = Secrets::new(store);

        let mut config = ConfigToml::default();
        config.providers.deepseek.api_key = Some("file-key".to_string());

        let resolved =
            config.resolve_runtime_options_with_secrets(&CliRuntimeOverrides::default(), &secrets);
        assert_eq!(resolved.api_key.as_deref(), Some("file-key"));

        // Safety: env mutation guarded by env_lock().
        unsafe { std::env::remove_var("DEEPSEEK_API_KEY") };
    }

    #[test]
    fn env_resolves_when_config_file_and_keyring_empty() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();
        // Safety: env mutation guarded by env_lock().
        unsafe { std::env::set_var("DEEPSEEK_API_KEY", "env-key") };

        let secrets = Secrets::new(std::sync::Arc::new(
            deepseek_secrets::InMemoryKeyringStore::new(),
        ));
        let config = ConfigToml::default();

        let resolved =
            config.resolve_runtime_options_with_secrets(&CliRuntimeOverrides::default(), &secrets);
        assert_eq!(resolved.api_key.as_deref(), Some("env-key"));

        // Safety: env mutation guarded by env_lock().
        unsafe { std::env::remove_var("DEEPSEEK_API_KEY") };
    }

    #[test]
    fn config_file_resolves_when_keyring_and_env_empty() {
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();

        let secrets = Secrets::new(std::sync::Arc::new(
            deepseek_secrets::InMemoryKeyringStore::new(),
        ));
        let mut config = ConfigToml::default();
        config.providers.deepseek.api_key = Some("file-key".to_string());

        let resolved =
            config.resolve_runtime_options_with_secrets(&CliRuntimeOverrides::default(), &secrets);
        assert_eq!(resolved.api_key.as_deref(), Some("file-key"));
    }

    #[test]
    fn cli_flag_still_overrides_keyring() {
        use deepseek_secrets::KeyringStore;
        let _lock = env_lock();
        let _env = EnvGuard::without_deepseek_runtime_overrides();

        let store = std::sync::Arc::new(deepseek_secrets::InMemoryKeyringStore::new());
        store.set("deepseek", "ring-key").unwrap();
        let secrets = Secrets::new(store);

        let cli = CliRuntimeOverrides {
            api_key: Some("cli-key".to_string()),
            ..CliRuntimeOverrides::default()
        };
        let resolved = ConfigToml::default().resolve_runtime_options_with_secrets(&cli, &secrets);
        assert_eq!(resolved.api_key.as_deref(), Some("cli-key"));
    }
}
