// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Federation records from the eduGAIN database API.
//!
//! `https://technical.edugain.org/api.php?action=list_feds_full` returns one record
//! per federation keyed by code. Its `reg_auth` joins directly to
//! `mdrpi:RegistrationInfo/@registrationAuthority` in entity metadata, which is how a
//! service's registrar becomes a named, country-tagged federation.

use std::collections::HashMap;

use anyhow::{bail, Result};
use serde_json::Value;

use crate::model::{Federation, FederationStats};

pub const EDUGAIN_FEDERATIONS_URL: &str =
    "https://technical.edugain.org/api.php?action=list_feds_full";

pub fn parse_edugain_federations(json: &[u8]) -> Result<Vec<Federation>> {
    let v: Value = serde_json::from_slice(json)?;
    let Some(map) = v.as_object() else {
        bail!("expected a JSON object keyed by federation code")
    };
    let mut feds: Vec<Federation> = map
        .iter()
        .filter_map(|(code, rec)| federation(code, rec))
        .collect();
    feds.sort_by_key(|f| f.name.to_lowercase());
    Ok(feds)
}

fn federation(code: &str, r: &Value) -> Option<Federation> {
    let reg_auth = str_of(r, "reg_auth")?;
    let status = match str_of(r, "status").as_deref() {
        Some("6") => "production".to_owned(),
        Some("1") => "candidate".to_owned(),
        Some("5") => "suspended".to_owned(),
        Some(other) => other.to_owned(),
        None => "unknown".to_owned(),
    };
    let stats = r.get("stats");
    Some(Federation {
        code: str_of(r, "code").unwrap_or_else(|| code.to_owned()),
        name: str_of(r, "name").unwrap_or_else(|| code.to_owned()),
        registration_authority: reg_auth,
        status,
        country_codes: strs_of(r, "country_code")
            .into_iter()
            .map(|c| c.to_uppercase())
            .collect(),
        countries: strs_of(r, "countries"),
        url: str_of(r, "url"),
        metadata_url: str_of(r, "metadata_url"),
        contact_email: str_of(r, "contact_email").or_else(|| str_of(r, "email")),
        security_contact_email: r
            .pointer("/security_contact/mail/value")
            .and_then(Value::as_str)
            .map(str::to_owned),
        registration_practice_urls: strs_of(r, "registration"),
        policy_urls: strs_of(r, "policy"),
        membership_date: str_of(r, "membership_date"),
        production_date: str_of(r, "production_date"),
        edugain_reported: FederationStats {
            entities: stats.and_then(|s| num_of(s, "All")),
            idps: stats.and_then(|s| num_of(s, "IdPs")),
            sps: stats.and_then(|s| num_of(s, "SPs")),
        },
    })
}

fn str_of(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn strs_of(v: &Value, k: &str) -> Vec<String> {
    match v.get(k) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn num_of(v: &Value, k: &str) -> Option<u64> {
    match v.get(k)? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Registration authorities appear with and without a trailing slash, and with
/// varying case in the host. Normalize both sides before joining.
pub fn normalize_reg_auth(s: &str) -> String {
    s.trim().trim_end_matches('/').to_lowercase()
}

pub struct FederationIndex<'a> {
    by_reg_auth: HashMap<String, &'a Federation>,
}

impl<'a> FederationIndex<'a> {
    pub fn new(feds: &'a [Federation]) -> Self {
        Self {
            by_reg_auth: feds
                .iter()
                .map(|f| (normalize_reg_auth(&f.registration_authority), f))
                .collect(),
        }
    }

    pub fn get(&self, reg_auth: &str) -> Option<&'a Federation> {
        self.by_reg_auth.get(&normalize_reg_auth(reg_auth)).copied()
    }
}
