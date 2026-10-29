// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Core of the federation service catalog.
//!
//! The pipeline is: stream-parse a signed SAML metadata aggregate ([`xml`]) one
//! `EntityDescriptor` at a time, reduce each service provider to a [`model::Service`]
//! ([`extract`]), merge the InCommon and eduGAIN views of the same entity ([`build`]),
//! and serve queries over the result ([`search`]).
//!
//! Everything here is pure and synchronous so it can run inside `spawn_blocking`, a
//! Lambda, or a test without an async runtime.

pub mod ask;
pub mod build;
pub mod categories;
pub mod curation;
pub mod environment;
pub mod examples;
pub mod extract;
pub mod federations;
pub mod model;
pub mod nl;
pub mod ns;
pub mod regions;
pub mod search;
pub mod sector;
pub mod sharing;
pub mod xml;

/// Bump when the snapshot layout changes incompatibly.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// The registration authority InCommon stamps on everything it registers.
pub const INCOMMON_REGISTRAR: &str = "https://incommon.org";

/// Catalog identifiers are the lowercase hex SHA-1 of the entityID — the same
/// transform the metadata query (MDQ) protocol uses for its `{sha1}` form, so an
/// ID from this API can be handed straight to `mdq.incommon.org`.
pub fn entity_hash(entity_id: &str) -> String {
    use sha1::{Digest, Sha1};
    hex::encode(Sha1::digest(entity_id.as_bytes()))
}
