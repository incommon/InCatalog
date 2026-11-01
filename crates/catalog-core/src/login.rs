// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! The Log in button on a service's page: whether it shows, and where it goes.
//!
//! It shows only on a service that is all of these, so the site and the API agree:
//!
//! - **multilateral**: its sharing label says any institution may use it;
//! - **alive**: its login hosts answer ([`crate::health`]);
//! - **not filtered by InCommon**: no `import_filter`, since an InCommon IdP can't
//!   see a service InCommon's import removed;
//! - **not OFAC-listed**: no entity match, possible or confirmed. A match curation
//!   dismissed is dropped from `ofac` already, and a country under an OFAC program
//!   doesn't count.
//!
//! Anything else gets no button, not even the fallback. Where it goes, best first:
//!
//! 1. **Wayfinder**: for a service in InCommon's aggregate, InCommon Wayfinder (built
//!    on SeamlessAccess), with the service's entityID and its own
//!    `idpdisc:DiscoveryResponse` endpoint as `return`. Wayfinder asks for the user's
//!    institution, remembers it on its own site, never ours, and sends the user to
//!    that endpoint, which starts the login. Wayfinder reads the service from
//!    InCommon's metadata, which carries both InCommon's registrations and the
//!    eduGAIN services it imports, and returns only to an endpoint published there.
//!    (InCommon's docs say "InCommon-registered", but on 2026-11-01 Wayfinder named a
//!    SWAMID-registered service InCommon imports, and named nothing for an entityID
//!    not in the metadata.) A service that expects to start discovery itself may not
//!    accept a choice it didn't ask for, so this is best-effort and the page says so.
//! 2. **SP-initiated**: the service's own login handler, its `init:RequestInitiator`
//!    endpoint (Shibboleth's `/Shibboleth.sso/Login`), for a service Wayfinder can't
//!    take. The service then asks for the user's institution itself.
//! 3. **Website**: its `mdui:InformationURL`, else its organization's URL. This is
//!    the service's front door, labelled as the site, not a login.
//!
//! Every URL comes from the service's signed metadata, never from a request, and must
//! be `https:` on the default port with a public host, so a registrant can't point
//! the button inside someone's network. Even then, getting in depends on the user's
//! institution releasing the attributes the service needs.

use serde::Serialize;
use url::{Host, Url};
use utoipa::ToSchema;

use crate::model::{pick, HealthStatus, Service, Source};
use crate::sharing::Sharing;

/// Whether a service's page shows a Log in button, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Login {
    /// Where the button goes. Absent when any of `blocked_by` holds, or when the
    /// metadata gives no usable URL.
    pub target: Option<Target>,
    /// Each condition that hides the button; empty when none does.
    pub blocked_by: Vec<Blocker>,
}

/// Where the button goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Target {
    pub route: Route,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Route {
    /// The service's own login handler (`init:RequestInitiator`).
    SpInitiated,
    /// InCommon Wayfinder, returning to the service's `idpdisc:DiscoveryResponse`.
    Wayfinder,
    /// The service's or its organization's website: not a login.
    Website,
}

/// A condition that hides the button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Blocker {
    /// Its sharing label is `bilateral` or `unknown`.
    NotMultilateral,
    /// Its health status is `dead` or `unknown`.
    NotAlive,
    /// InCommon's eduGAIN import filtered it out.
    FilteredByIncommon,
    /// Its organization matched an OFAC list entry, possibly or confirmed.
    OfacMatch,
}

/// The button for a service, given its sharing label.
pub fn login(s: &Service, sharing: Sharing) -> Login {
    let mut blocked_by = Vec::new();
    if sharing != Sharing::Multilateral {
        blocked_by.push(Blocker::NotMultilateral);
    }
    if crate::health::status(s) != HealthStatus::Alive {
        blocked_by.push(Blocker::NotAlive);
    }
    if s.import_filter.is_some() {
        blocked_by.push(Blocker::FilteredByIncommon);
    }
    if !s.ofac.is_empty() {
        blocked_by.push(Blocker::OfacMatch);
    }
    let target = if blocked_by.is_empty() {
        target(s)
    } else {
        None
    };
    Login { target, blocked_by }
}

fn target(s: &Service) -> Option<Target> {
    let sp_initiated = s
        .request_initiators
        .iter()
        .filter(|e| e.binding == crate::ns::INIT)
        .find_map(|e| safe_link(&e.location))
        .map(|url| Target {
            route: Route::SpInitiated,
            url,
        });
    let website = || {
        [&s.information_urls, &s.organization.urls]
            .into_iter()
            .find_map(|m| pick(m, Some("en")).and_then(safe_link))
            .map(|url| Target {
                route: Route::Website,
                url,
            })
    };
    wayfinder(s).or(sp_initiated).or_else(website)
}

/// InCommon Wayfinder's SAML discovery service.
pub const WAYFINDER: &str = "https://wayfinder.incommon.org/";

/// Wayfinder for a service in InCommon's aggregate with a discovery response endpoint.
/// `return` is the endpoint exactly as registered, since Wayfinder compares it with
/// the metadata; it only has to pass [`safe_link`].
fn wayfinder(s: &Service) -> Option<Target> {
    if !s.sources.contains(&Source::Incommon) {
        return None;
    }
    let ret = s
        .discovery_responses
        .iter()
        .filter(|e| e.binding == crate::ns::IDPDISC)
        .map(|e| e.location.trim())
        .find(|l| safe_link(l).is_some())?;
    let mut url = Url::parse(WAYFINDER).expect("a valid URL");
    url.query_pairs_mut()
        .append_pair("entityID", &s.entity_id)
        .append_pair("return", ret);
    Some(Target {
        route: Route::Wayfinder,
        url: url.into(),
    })
}

/// The URL, normalized, when a browser may be sent there: `https:` on the default
/// port, no user or password, and a public address or a dotted name that isn't
/// local.
pub fn safe_link(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let host_ok = match url.host()? {
        Host::Ipv4(ip) => crate::net::is_public(ip.into()),
        Host::Ipv6(ip) => crate::net::is_public(ip.into()),
        Host::Domain(d) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d.contains('.')
                && ![".localhost", ".local", ".internal", ".home.arpa"]
                    .iter()
                    .any(|suffix| d.ends_with(suffix))
        }
    };
    host_ok.then(|| url.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Endpoint, Health, ImportFilter, OfacMatch};
    use crate::ofac::{List, MatchStatus};

    const LOGIN: &str = "https://sp.example.edu/Shibboleth.sso/Login";

    /// A service that passes all four conditions, with a login handler and a site.
    fn open() -> Service {
        let mut s = Service {
            entity_id: "https://sp.example.edu/shibboleth".into(),
            health: Some(Health {
                status: HealthStatus::Alive,
                ..Default::default()
            }),
            request_initiators: vec![Endpoint {
                location: LOGIN.into(),
                binding: crate::ns::INIT.into(),
            }],
            ..Default::default()
        };
        s.information_urls
            .insert("en".into(), "https://example.edu/about".into());
        s.organization
            .urls
            .insert("en".into(), "https://example.edu/".into());
        s
    }

    fn ofac_match(status: MatchStatus) -> OfacMatch {
        OfacMatch {
            status,
            list: List::Sdn,
            uid: "1".into(),
            name: "Example".into(),
            programs: vec![],
            matched_on: vec![],
            list_published: None,
            decided: None,
            note: None,
        }
    }

    fn target_of(s: &Service) -> Option<(Route, String)> {
        login(s, Sharing::Multilateral)
            .target
            .map(|t| (t.route, t.url))
    }

    #[test]
    fn sp_initiated_through_the_request_initiator() {
        assert_eq!(target_of(&open()), Some((Route::SpInitiated, LOGIN.into())));
        assert!(login(&open(), Sharing::Multilateral).blocked_by.is_empty());
    }

    #[test]
    fn a_request_initiator_with_another_binding_is_not_used() {
        let mut s = open();
        s.request_initiators[0].binding = "urn:example:other".into();
        assert_eq!(
            target_of(&s),
            Some((Route::Website, "https://example.edu/about".into()))
        );
    }

    #[test]
    fn falls_back_to_the_information_url_then_the_organization() {
        let mut s = open();
        s.request_initiators.clear();
        assert_eq!(
            target_of(&s),
            Some((Route::Website, "https://example.edu/about".into()))
        );
        s.information_urls
            .insert("en".into(), "http://example.edu/about".into());
        assert_eq!(
            target_of(&s),
            Some((Route::Website, "https://example.edu/".into()))
        );
        s.organization.urls.clear();
        assert_eq!(target_of(&s), None);
        assert!(login(&s, Sharing::Multilateral).blocked_by.is_empty());
    }

    #[test]
    fn an_unsafe_login_handler_falls_back_to_the_site() {
        let mut s = open();
        s.request_initiators[0].location = "https://127.0.0.1/Shibboleth.sso/Login".into();
        assert_eq!(target_of(&s).unwrap().0, Route::Website);
    }

    #[test]
    fn each_condition_alone_hides_the_button_and_the_fallback() {
        type Break = fn(&mut Service, &mut Sharing);
        let cases: [(Blocker, Break); 7] = [
            (Blocker::NotMultilateral, |_, sh| *sh = Sharing::Bilateral),
            (Blocker::NotMultilateral, |_, sh| *sh = Sharing::Unknown),
            (Blocker::NotAlive, |s, _| s.health = None),
            (Blocker::NotAlive, |s, _| {
                s.health.as_mut().unwrap().status = HealthStatus::Dead
            }),
            (Blocker::FilteredByIncommon, |s, _| {
                s.import_filter = Some(ImportFilter::default())
            }),
            (Blocker::OfacMatch, |s, _| {
                s.ofac = vec![ofac_match(MatchStatus::Possible)]
            }),
            (Blocker::OfacMatch, |s, _| {
                s.ofac = vec![ofac_match(MatchStatus::Confirmed)]
            }),
        ];
        for (blocker, break_it) in cases {
            for keep_login in [true, false] {
                let mut s = open();
                if !keep_login {
                    s.request_initiators.clear();
                }
                let mut sharing = Sharing::Multilateral;
                break_it(&mut s, &mut sharing);
                let l = login(&s, sharing);
                assert_eq!(l.target, None, "{blocker:?}");
                assert_eq!(l.blocked_by, vec![blocker]);
            }
        }
    }

    #[test]
    fn every_failing_condition_is_listed() {
        let mut s = open();
        s.health = None;
        s.import_filter = Some(ImportFilter::default());
        s.ofac = vec![ofac_match(MatchStatus::Possible)];
        assert_eq!(
            login(&s, Sharing::Bilateral).blocked_by,
            vec![
                Blocker::NotMultilateral,
                Blocker::NotAlive,
                Blocker::FilteredByIncommon,
                Blocker::OfacMatch
            ]
        );
    }

    #[test]
    fn only_public_https_urls_on_the_default_port() {
        for bad in [
            "http://sp.example.edu/Login",
            "javascript:alert(1)",
            "data:text/html,hi",
            "//evil.example/Login",
            "/Shibboleth.sso/Login",
            "https://localhost/Login",
            "https://sp.localhost/Login",
            "https://printer.local/Login",
            "https://intranet/Login",
            "https://db.corp.internal/Login",
            "https://127.0.0.1/Login",
            "https://2130706433/Login",
            "https://10.0.0.5/Login",
            "https://169.254.169.254/latest/meta-data/",
            "https://[::1]/Login",
            "https://[fc00::1]/Login",
            "https://sp.example.edu:8443/Login",
            "https://user:pw@sp.example.edu/Login",
            "https://sp.example.edu@evil.example/Login",
            "",
            "not a url",
        ] {
            assert_eq!(safe_link(bad), None, "{bad}");
        }
        assert_eq!(
            safe_link(" https://SP.Example.edu:443/Shibboleth.sso/Login?entityID=x "),
            Some("https://sp.example.edu/Shibboleth.sso/Login?entityID=x".into())
        );
        assert_eq!(
            safe_link("https://8.8.8.8/x"),
            Some("https://8.8.8.8/x".into())
        );
    }

    /// In InCommon's aggregate, with a discovery response endpoint and no login handler.
    fn incommon() -> Service {
        let mut s = open();
        s.request_initiators.clear();
        s.sources = vec![Source::Incommon];
        s.discovery_responses = vec![Endpoint {
            location: LOGIN.into(),
            binding: crate::ns::IDPDISC.into(),
        }];
        s
    }

    fn query(url: &str) -> Vec<(String, String)> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    #[test]
    fn wayfinder_for_an_incommon_registration() {
        assert_eq!(
            target_of(&incommon()),
            Some((
                Route::Wayfinder,
                "https://wayfinder.incommon.org/?entityID=https%3A%2F%2Fsp.example.edu%2Fshibboleth\
                 &return=https%3A%2F%2Fsp.example.edu%2FShibboleth.sso%2FLogin"
                    .into()
            ))
        );
    }

    #[test]
    fn wayfinder_for_any_service_in_incommons_aggregate() {
        // Another federation's registration that InCommon imports still gets Wayfinder.
        let mut s = incommon();
        s.registration.authority = Some("http://www.swamid.se/".into());
        s.sources = vec![Source::Incommon, Source::Edugain];
        assert_eq!(target_of(&s).unwrap().0, Route::Wayfinder);
        // Only in eduGAIN's: Wayfinder can't read it.
        s.sources = vec![Source::Edugain];
        assert_eq!(target_of(&s).unwrap().0, Route::Website);
    }

    #[test]
    fn wayfinder_comes_before_the_login_handler() {
        let mut s = incommon();
        s.request_initiators = open().request_initiators;
        assert_eq!(target_of(&s).unwrap().0, Route::Wayfinder);
        // Without a usable discovery response endpoint, the login handler.
        s.discovery_responses.clear();
        assert_eq!(target_of(&s), Some((Route::SpInitiated, LOGIN.into())));
    }

    #[test]
    fn wayfinder_returns_only_to_a_safe_endpoint_as_registered() {
        let mut s = incommon();
        s.discovery_responses = vec![
            Endpoint {
                location: "https://sp.example.edu/other".into(),
                binding: "urn:example:other".into(),
            },
            Endpoint {
                location: "https://10.0.0.5/Shibboleth.sso/Login".into(),
                binding: crate::ns::IDPDISC.into(),
            },
            Endpoint {
                location: " https://SP.example.edu:443/Shibboleth.sso/Login ".into(),
                binding: crate::ns::IDPDISC.into(),
            },
        ];
        let (route, url) = target_of(&s).unwrap();
        assert_eq!(route, Route::Wayfinder);
        // Wayfinder compares `return` with the metadata, so it isn't normalized.
        assert_eq!(
            query(&url)[1],
            (
                "return".into(),
                "https://SP.example.edu:443/Shibboleth.sso/Login".into()
            )
        );
        s.discovery_responses.remove(2);
        assert_eq!(target_of(&s).unwrap().0, Route::Website);
    }

    #[test]
    fn a_crafted_entity_id_stays_one_parameter() {
        let mut s = incommon();
        s.entity_id = "https://sp.example.edu/?x=1&return=https://evil.example/#".into();
        let (_, url) = target_of(&s).unwrap();
        assert!(url.starts_with(WAYFINDER));
        assert_eq!(
            query(&url),
            [
                ("entityID".to_owned(), s.entity_id.clone()),
                ("return".to_owned(), LOGIN.to_owned())
            ]
        );
    }

    #[test]
    fn the_entity_id_is_never_a_destination() {
        // A service whose only https URL is a crafted entityID gets no target: the
        // button goes only where an endpoint or a link in its metadata says.
        let mut s = open();
        s.entity_id = "https://evil.example/?next=https://evil.example".into();
        s.request_initiators.clear();
        s.information_urls.clear();
        s.organization.urls.clear();
        assert_eq!(target_of(&s), None);
    }
}
