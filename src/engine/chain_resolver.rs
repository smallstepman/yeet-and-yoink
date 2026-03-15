// Re-export shim — contents have been migrated to engine::resolution.
// Preserved for backward compatibility: callers that import from engine::chain_resolver.
pub(crate) use crate::engine::resolution::{
    RuntimeChainResolver,
    runtime_chain_resolver,
    resolve_app_chain,
    default_app_domain_adapters,
    resolve_window_domain_id,
};
