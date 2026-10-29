// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! XML namespace URIs used in SAML metadata.
//!
//! Every lookup in [`crate::extract`] is namespace-qualified. An unqualified lookup
//! silently finds nothing, and "nothing" reads as a zero count.

pub const MD: &str = "urn:oasis:names:tc:SAML:2.0:metadata";
pub const SAML: &str = "urn:oasis:names:tc:SAML:2.0:assertion";
pub const MDRPI: &str = "urn:oasis:names:tc:SAML:metadata:rpi";
pub const MDATTR: &str = "urn:oasis:names:tc:SAML:metadata:attribute";
pub const MDUI: &str = "urn:oasis:names:tc:SAML:metadata:ui";
pub const IDPDISC: &str = "urn:oasis:names:tc:SAML:profiles:SSO:idp-discovery-protocol";
pub const XML: &str = "http://www.w3.org/XML/1998/namespace";
/// REFEDS metadata extensions. SAML's `contactType` enumeration has no "security"
/// member, so the REFEDS security contact is `contactType="other"` plus
/// `remd:contactType="http://refeds.org/metadata/contactType/security"`.
pub const REMD: &str = "http://refeds.org/metadata";

pub const ATTR_ENTITY_CATEGORY: &str = "http://macedir.org/entity-category";
pub const ATTR_ENTITY_CATEGORY_SUPPORT: &str = "http://macedir.org/entity-category-support";
pub const ATTR_ASSURANCE_CERTIFICATION: &str =
    "urn:oasis:names:tc:SAML:attribute:assurance-certification";
pub const REFEDS_SECURITY_CONTACT: &str = "http://refeds.org/metadata/contactType/security";
