// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Example searches, one or more for each thing the search understands. The landing
//! page offers a few of them and the About page lists them all (both through
//! `/v1/examples`), and `tests/examples.rs` reads every one with the rules and
//! checks it finds what it should, so an example cannot quietly stop working.

use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Example {
    /// What a person types.
    pub query: &'static str,
    /// The search behavior it shows, in a few words.
    pub shows: &'static str,
    /// The filters the rules read it as, by `/v1/services` parameter. `q` is the
    /// words left over; a query with no `q` here must leave none.
    #[serde(skip)]
    pub reads_as: &'static [(&'static str, &'static str)],
}

const MAIL: &str = "urn:oid:0.9.2342.19200300.100.1.3";
const ORCID: &str = "urn:oid:1.3.6.1.4.1.5923.1.1.1.16";

pub const EXAMPLES: &[Example] = &[
    Example {
        query: "R&S services in Sweden that request email",
        shows: "a category, a country, and a requested attribute",
        reads_as: &[
            ("category", "research-and-scholarship"),
            ("country", "SE"),
            ("attribute", MAIL),
        ],
    },
    Example {
        query: "services registered by SWAMID or DFN-AAI",
        shows: "several federations at once",
        reads_as: &[("federation", "SWAMID,DFN-AAI")],
    },
    Example {
        query: "UK federation services that need ORCID",
        shows: "a federation and a requested attribute",
        reads_as: &[("federation", "UK-FEDERATION"), ("attribute", ORCID)],
    },
    Example {
        query: "journals not in InCommon",
        shows: "where a service can be used from",
        reads_as: &[("availability", "edugain-only"), ("q", "journals")],
    },
    Example {
        query: "anonymous access services exported to eduGAIN",
        shows: "an entity category and availability",
        reads_as: &[
            ("category", "anonymous"),
            ("availability", "incommon-and-edugain"),
        ],
    },
    Example {
        query: "test instances of Canvas",
        shows: "test, staging, and development instances",
        reads_as: &[("environment", "test"), ("q", "canvas")],
    },
    Example {
        query: "only production services that request ORCID",
        shows: "live services only",
        reads_as: &[("environment", "production"), ("attribute", ORCID)],
    },
    Example {
        query: "R&S services in Europe",
        shows: "a geographic region",
        reads_as: &[
            ("category", "research-and-scholarship"),
            ("region", "europe"),
        ],
    },
    Example {
        query: "services in Northern Europe",
        shows: "a UN M49 sub-region",
        reads_as: &[("region", "northern-europe")],
    },
    Example {
        query: "R&S services in Scandinavia",
        shows: "a region by its common name",
        reads_as: &[
            ("category", "research-and-scholarship"),
            ("region", "scandinavia"),
        ],
    },
    Example {
        query: "SPs in the EU that request email",
        shows: "a political grouping",
        reads_as: &[("region", "eu"), ("attribute", MAIL)],
    },
    Example {
        query: "anything from Schengen countries",
        shows: "a grouping that is not the EU",
        reads_as: &[("region", "schengen")],
    },
    Example {
        query: "commercial vendors in Europe",
        shows: "who runs a service",
        reads_as: &[("sector", "commercial"), ("region", "europe")],
    },
    Example {
        query: "services anyone in eduGAIN can use",
        shows: "services open to any institution",
        reads_as: &[("sharing", "multilateral")],
    },
];
