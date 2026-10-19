//! The native host (design note 0030, §9): the stock binary has no spores; this
//! workspace's first-party ones are built in by a feature (`--features spore-stripe`). A
//! deployment with others builds its own crate with the same line, listing them.

#[cfg(not(feature = "spore-stripe"))]
hypha_host_axum::host! { spores: [] }

#[cfg(feature = "spore-stripe")]
hypha_host_axum::host! {
    spores: [hypha_spore_stripe::Stripe],
    routes: [hypha_spore_stripe::stripe_routes],
}
