/// Kernel port for the tenant's plugin catalog-changed notices.
///
/// The downlink dispatcher hands each notice here; the SDK facade owns the
/// implementation (invalidate the catalog, then announce the change to hosts
/// after a random delay, merging the notices that arrive meanwhile).
pub trait PluginCatalogNoticePort: Send + Sync {
    /// Core accepted a newer plugin authorization projection `version` for
    /// this user's tenant.
    fn catalog_changed(&self, version: String);
}
