use anyhow::Result;
use std::{fs, path::Path};

use crate::{emit_errors, fail};

pub(crate) fn verify_enterprise_compliance_gate(root: &Path) -> Result<()> {
    let mut errors = Vec::new();
    let monorepo_root = root.parent().unwrap_or_else(|| Path::new(".."));

    require_contains_all(
        &mut errors,
        &root.join("docs/enterprise-compliance.md"),
        "enterprise compliance doc",
        &[
            "Admin Write Audit Contract",
            "`x-audit-reason`",
            "`idempotency-key`",
            "Enterprise Policy Discovery",
            "`organization_policy`",
            "`data_residency_policy`",
            "`retention_legal_policy`",
            "capability_audit_log",
            "compliance-text-check",
            "audit-log-sink",
            "recall-audit",
        ],
    );
    require_contains_all(
        &mut errors,
        &monorepo_root
            .join("flare-im-core/flare-admin-gateway/src/interface/http/admin_auth_middleware.rs"),
        "admin gateway audit middleware",
        &[
            "has_gateway_admin_scope",
            "tenant context is required for admin api",
            "admin tenant context does not match authenticated principal",
            "actor is required for admin write api",
            "audit reason is required for admin write api",
            "x-request-id or idempotency-key is required for admin write api",
            "admin_write_requires_audit_context",
            "admin_authorization_rejects_tenant_mismatch",
        ],
    );
    require_contains_all(
        &mut errors,
        &monorepo_root
            .join("flare-im-core/flare-admin-gateway/src/interface/http/admin_contract.rs"),
        "admin capability descriptor",
        &[
            "AUDIT_REASON_HEADER",
            "IDEMPOTENCY_KEY_HEADER",
            "AdminRequiredHeaders",
            "EnterprisePolicyStatus",
            "EnterprisePolicyAuthority",
            "OrganizationRoleSource",
            "EnterpriseProtectedOperation",
            "RetentionEnforcementAnchor",
            "AdminOrganizationPolicyDescriptor",
            "AdminDataResidencyPolicyDescriptor",
            "AdminRetentionLegalPolicyDescriptor",
            "organization_policy",
            "data_residency_policy",
            "retention_legal_policy",
            "audit_reason_header",
            "idempotency_key_header",
            "EnterprisePolicyAuthority::TenantRegionPolicyProvider",
            "EnterprisePolicyAuthority::EnterpriseRetentionPolicyProvider",
            "EnterpriseProtectedOperation::MessageExport",
            "RetentionEnforcementAnchor::CapabilityAuditLog",
        ],
    );
    require_contains_all(
        &mut errors,
        &monorepo_root.join("flare-im-core/deploy/init.sql"),
        "capability audit schema",
        &[
            "CREATE TABLE capability_audit_log",
            "action TEXT NOT NULL",
            "tenant_id TEXT NOT NULL",
            "actor_id TEXT",
            "detail JSONB",
            "idx_capability_audit_tenant_time",
            "idx_capability_audit_action_time",
        ],
    );
    require_contains_all(
        &mut errors,
        &monorepo_root.join(
            "flare-im-core/flare-capability/src/infrastructure/persistence/postgres_capability_audit.rs",
        ),
        "capability audit writer",
        &[
            "PostgresCapabilityAuditLog",
            "record_policy_event",
            "INSERT INTO public.capability_audit_log",
            "tracing::error!",
        ],
    );
    require_contains_all(
        &mut errors,
        &monorepo_root.join("flare-im-core/config/hooks.example.toml"),
        "message compliance hook example",
        &[
            "compliance-text-check",
            "require_success = true",
            "audit-log-sink",
            "post_send",
            "recall-audit",
        ],
    );

    emit_errors("enterprise compliance gate", errors)
}

fn require_contains_all(errors: &mut Vec<String>, path: &Path, label: &str, needles: &[&str]) {
    let Ok(text) = fs::read_to_string(path) else {
        fail(
            errors,
            format!("{label} missing or unreadable: {}", path.display()),
        );
        return;
    };

    for needle in needles {
        if !text.contains(needle) {
            fail(
                errors,
                format!("{label} missing `{needle}` in {}", path.display()),
            );
        }
    }
}
