//! The native host: the stock binary has no spores. A deployment with spores builds its
//! own crate with the same line, listing them (`hypha deploy` writes it).

hypha_host_axum::host! { spores: [] }
