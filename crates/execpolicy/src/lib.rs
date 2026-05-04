pub mod bash_arity;

use std::collections::HashSet;

use anyhow::Result;
use bash_arity::BashArityDict;
use deepseek_protocol::{NetworkPolicyAmendment, NetworkPolicyRuleAction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AskForApproval {
    UnlessTrusted,
    OnFailure,
    OnRequest,
    Reject {
        sandbox_approval: bool,
        rules: bool,
        mcp_elicitations: bool,
    },
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecPolicyAmendment {
    pub prefixes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecApprovalRequirement {
    Skip {
        bypass_sandbox: bool,
        proposed_execpolicy_amendment: Option<ExecPolicyAmendment>,
    },
    NeedsApproval {
        reason: String,
        proposed_execpolicy_amendment: Option<ExecPolicyAmendment>,
        proposed_network_policy_amendments: Vec<NetworkPolicyAmendment>,
    },
    Forbidden {
        reason: String,
    },
}

impl ExecApprovalRequirement {
    pub fn reason(&self) -> &str {
        match self {
            ExecApprovalRequirement::Skip { .. } => "Execution allowed by policy.",
            ExecApprovalRequirement::NeedsApproval { reason, .. } => reason,
            ExecApprovalRequirement::Forbidden { reason } => reason,
        }
    }

    pub fn phase(&self) -> &'static str {
        match self {
            ExecApprovalRequirement::Skip { .. } => "allowed",
            ExecApprovalRequirement::NeedsApproval { .. } => "needs_approval",
            ExecApprovalRequirement::Forbidden { .. } => "forbidden",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecPolicyDecision {
    pub allow: bool,
    pub requires_approval: bool,
    pub requirement: ExecApprovalRequirement,
    pub matched_rule: Option<String>,
}

impl ExecPolicyDecision {
    pub fn reason(&self) -> &str {
        self.requirement.reason()
    }
}

#[derive(Debug, Clone)]
pub struct ExecPolicyContext<'a> {
    pub command: &'a str,
    pub cwd: &'a str,
    pub ask_for_approval: AskForApproval,
    pub sandbox_mode: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct ExecPolicyEngine {
    trusted_prefixes: Vec<String>,
    denied_prefixes: Vec<String>,
    approved_for_session: HashSet<String>,
    /// Arity dictionary for command-prefix allow-rule matching.
    arity_dict: BashArityDict,
}

impl Default for ExecPolicyEngine {
    fn default() -> Self {
        Self {
            trusted_prefixes: Vec::new(),
            denied_prefixes: Vec::new(),
            approved_for_session: HashSet::new(),
            arity_dict: BashArityDict::new(),
        }
    }
}

impl ExecPolicyEngine {
    pub fn new(trusted_prefixes: Vec<String>, denied_prefixes: Vec<String>) -> Self {
        Self {
            trusted_prefixes,
            denied_prefixes,
            approved_for_session: HashSet::new(),
            arity_dict: BashArityDict::new(),
        }
    }

    pub fn remember_session_approval(&mut self, approval_key: String) {
        self.approved_for_session.insert(approval_key);
    }

    pub fn is_session_approved(&self, approval_key: &str) -> bool {
        self.approved_for_session.contains(approval_key)
    }

    pub fn check(&self, ctx: ExecPolicyContext<'_>) -> Result<ExecPolicyDecision> {
        let normalized = normalize_command(ctx.command);
        // Deny rules use simple prefix matching (no arity semantics needed).
        if let Some(rule) = self
            .denied_prefixes
            .iter()
            .find(|rule| normalized.starts_with(&normalize_command(rule)))
        {
            return Ok(ExecPolicyDecision {
                allow: false,
                requires_approval: false,
                matched_rule: Some(rule.clone()),
                requirement: ExecApprovalRequirement::Forbidden {
                    reason: format!("Command blocked by denied prefix rule '{rule}'"),
                },
            });
        }

        // Allow (trusted) rules use arity-aware prefix matching so that
        // `auto_allow = ["git status"]` matches `git status -s` but NOT
        // `git push origin main`.
        let trusted_rule = self
            .trusted_prefixes
            .iter()
            .find(|rule| self.arity_dict.allow_rule_matches(rule, ctx.command))
            .cloned();
        let is_trusted = trusted_rule.is_some();

        let requirement = match ctx.ask_for_approval {
            AskForApproval::Never => ExecApprovalRequirement::Skip {
                bypass_sandbox: false,
                proposed_execpolicy_amendment: None,
            },
            AskForApproval::UnlessTrusted if is_trusted => ExecApprovalRequirement::Skip {
                bypass_sandbox: false,
                proposed_execpolicy_amendment: None,
            },
            AskForApproval::OnFailure => ExecApprovalRequirement::Skip {
                bypass_sandbox: false,
                proposed_execpolicy_amendment: None,
            },
            AskForApproval::Reject { rules, .. } if rules => ExecApprovalRequirement::Forbidden {
                reason: "Policy is configured to reject rule-exceptions.".to_string(),
            },
            _ => ExecApprovalRequirement::NeedsApproval {
                reason: if is_trusted {
                    "Approval requested by policy mode.".to_string()
                } else {
                    "Unmatched command prefix requires approval.".to_string()
                },
                proposed_execpolicy_amendment: if is_trusted {
                    None
                } else {
                    Some(ExecPolicyAmendment {
                        prefixes: vec![first_token(ctx.command)],
                    })
                },
                proposed_network_policy_amendments: vec![NetworkPolicyAmendment {
                    host: ctx.cwd.to_string(),
                    action: NetworkPolicyRuleAction::Allow,
                }],
            },
        };

        let (allow, requires_approval) = match requirement {
            ExecApprovalRequirement::Skip { .. } => (true, false),
            ExecApprovalRequirement::NeedsApproval { .. } => (true, true),
            ExecApprovalRequirement::Forbidden { .. } => (false, false),
        };

        Ok(ExecPolicyDecision {
            allow,
            requires_approval,
            matched_rule: trusted_rule,
            requirement,
        })
    }
}

fn normalize_command(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn first_token(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}
