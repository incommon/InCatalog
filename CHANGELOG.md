# Changelog

All notable changes to InCatalog are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as CONTRIBUTING.md
§ Versioning describes. The API embeds this file when it is built and serves it at
`/v1/releases`, and the site shows the notes for the running version, so a release's
section must be written before it is cut.

## [Unreleased]

## [0.2.0] - 2026-10-29

### Added

- **Several federations at once.** `federation` on `/v1/services` takes a comma list
  or repeated parameters, and matches a service registered by any of them. The
  "Registered by" facet is a multi-select, and plain-language searches keep every
  federation they name ("registered by SWAMID or DFN-AAI"). (#9)
- **Production, test, staging and development.** Every service carries an
  `environment`, read from markers in its entityID, names and endpoints, or else from
  Claude's summary, with where the reading came from. `environment` is a filter and a
  facet, and "test instances of Canvas" reads as one. (#14)
- **Regions.** `region` on `/v1/services` takes UN M49 regions and dated political
  groupings, and `country` takes several countries; together they are one set of
  places. `/v1/regions` lists every region with its countries and service count, and
  results say what each region meant. A federation is in a region when any of its
  countries is. (#15)
- **Multilateral and bilateral services.** Each service is labelled `multilateral`,
  `bilateral` or `unknown`: tenant clusters outside eduGAIN are bilateral, Claude
  labels the rest in ingest, and R&S or REFEDS access categories in eduGAIN suggest
  multilateral where Claude has not answered. `sharing` is a filter and a facet, and
  service pages say who a service is open to. "Multilateral" never promises access:
  that still depends on attribute release and any subscription. (#16)
- **More groupings, kept as dated data.** Schengen, the euro area, the Nordic Council,
  NATO, Five Eyes, the OECD, the G7 and G20, ASEAN, APEC, Mercosur, USMCA, the African
  Union, the GCC, the Commonwealth and the research-network regions (GÉANT, RedCLARA,
  APAN, WACREN, UbuntuNet, ASREN) join the UN M49 regions, the EU, the EEA and BRICS in
  `crates/catalog-core/data/regions.json`, each with its source and the date it was
  checked. A test fails once a list is a year old. (#20)
- **Africa's regional blocs.** ECOWAS, SADC, EAC, COMESA, ECCAS, IGAD, CEN-SAD, the Arab
  Maghreb Union, WAEMU, CEMAC, the CFA franc zone, SACU, AfCFTA state parties and the
  Arab League. A region's chip lists members that left, with the date they left. (#22)
- **Who runs a service.** Each service carries the `sectors` of the organization that
  registered it (academia, research, commercial, libraries, museums, government,
  healthcare, K–12, publishers, nonprofit, NRENs), from the curation overlay, rules
  over the organization's name and domain, or else one Claude reading per
  organization, with `sector_source` saying which. `sector` is a filter and a facet,
  and "government agencies" or "commercial vendors in Europe" read as one. (#23)
- **Example searches.** `/v1/examples` serves one tested example for each search
  behavior; the landing page rotates through them and About lists them all with a
  "How to search" guide. A test reads every example and fails if one stops finding
  what it shows. (#21)
- **Release notes.** `/v1/meta` reports the running `version`, `/v1/releases` and
  `/v1/releases/{version}` serve this changelog, and the footer links the version to
  its notes.

### Changed

- The metadata parser moved to quick-xml 0.42, which rejects an aggregate that is not
  valid UTF-8 as a whole rather than patching it field by field. Parsing every entity
  in the InCommon and eduGAIN aggregates gives the same result as before. (#1, #11)
- Logos: a failed website lookup is retried within a run, transient failures (DNS,
  timeouts, TLS) are retried after six hours instead of three days, a run stops
  looking up after twelve unreachable sites in a row, and the per-run cap rose from
  150 to 300. Ingest reports why each logo is missing. (#18)
- "Library services" and "library SPs" now mean services run by libraries
  (`sector=libraries`), and "museum services" and "museum SPs" services run by museums.
  "Library" or "museum" alone still searches the word, so "library management system"
  is a subject. "R&S library services in Sweden" changes meaning with this. (#26)
- Dependencies: lambda_http 1.3, lambda_runtime 1.5, tower-http 0.7, utoipa 6, sha1
  0.11 and sha2 0.11. (#2, #3, #5–#8)

### Fixed

- Every grouping in `regions.json` is now checked against a page read from its
  source, and its `url` points at that page. Dates the source gives only to the month
  or year are no longer given to the day. Corrections include the UK leaving the EU on
  2020-01-31 (recorded in `left`), Morocco rejoining the African Union on 2017-01-31,
  and the G7's current membership dating from Russia's suspension in March 2014. (#25)
- The image's smoke test can read its snapshot on Linux runners. (#12)

### Security

- CI runs formatting, clippy, the tests and a ratchet-only coverage gate, checks
  Terraform, and holds dependencies to a permissive license allow-list. A weekly
  RustSec scan and Dependabot watch the dependencies. Ten advisories found by the
  first scan are resolved. (#1)
- The image ships the license, the attribution notice and the dependencies' license
  texts, generated from the `Cargo.lock` it is built from. (#10)

## [0.1.0] - 2026-10-29

### Added

- A searchable catalog of the SAML service providers in the InCommon and eduGAIN
  metadata aggregates: ingest (with signature checks), a JSON API with an OpenAPI
  document, plain-language search, providers, federations, categories and requested
  attributes, and a static web front end. It runs locally with `make dev` and in AWS
  as two Lambda functions from one container image.

[Unreleased]: https://github.com/incommon/InCatalog/compare/v0.2.0...develop
[0.2.0]: https://github.com/incommon/InCatalog/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/incommon/InCatalog/tree/v0.1.0
