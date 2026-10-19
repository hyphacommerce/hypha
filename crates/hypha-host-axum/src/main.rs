//! The stock native host: no spores (design note 0030, §9). A deployment with spores
//! builds its own crate with the same line, listing them.

hypha_host_axum::host! { spores: [] }
