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
        // 中间件已从「令牌带 admin scope」改为验签的员工控制令牌（admin 用途、core-admin
        // 受众），租户与操作者都按令牌核对；原来点名的 `has_gateway_admin_scope`、两条报错
        // 文案和两个用例名随之换掉，这里按新实现逐条点名，判据不少于原来。
        &[
            "verifier.verify_token(token, \"admin\", CORE_ADMIN_AUDIENCE)",
            "actor.require_kind(ControlActorKind::Employee)",
            "tenant context is required for admin api",
            "one concrete tenant is required for admin api",
            "actor.require_tenant(&tenant_id)",
            "admin actor header does not match token",
            "verified actor header is required for admin write api",
            "audit reason is required for admin write api",
            "x-request-id or idempotency-key is required for admin write api",
            "admin_boundary_accepts_only_signed_employee_admin_audience",
            "admin_read_requires_one_exact_tenant",
            "admin_write_requires_verified_actor_and_audit_context",
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
