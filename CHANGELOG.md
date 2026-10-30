# Changelog

All notable changes to InCatalog are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as CONTRIBUTING.md
§ Versioning describes. The API embeds this file when it is built and serves it at
`/v1/releases`, and the site shows the notes for the running version, so a release's
section must be written before it is cut.

## [Unreleased]

## [0.3.0] - 2026-10-30

### Added

- `make backfill` runs ingest in AWS until nothing is waiting, one run at a time and
  never overlapping the scheduled run, for working off a backlog in hours rather than
  days. (#33)
- **Regions by their common names.** The Middle East, Central and Eastern Europe, the
  Balkans, the Western Balkans, Scandinavia, the Baltic states, the Pacific Islands,
  the Horn of Africa, the Sahel, and the Maghreb, each from one cited source with a
  note on what it includes, and SAARC as a dated grouping. Asia-Pacific moves to
  this new `common` region kind, since UN M49 does not define it. "Pacific Islands"
  now means Melanesia, Micronesia, and Polynesia rather than all of Oceania. (#34)
- `/v1/regions` gives each UN M49 sub-region its `parent`, so the regions form one
  tree under the five continents. (#34)

### Changed

- Infrastructure is applied with OpenTofu 1.12.5 instead of Terraform, as
  `.opentofu-version` pins; `make init` and everything after it refuse any other
  version. The first `tofu apply` takes over the state, so `terraform` must not be run
  against it again. (#35)

### Fixed

- **A plural search word no longer matches longer words through its singular.**
  "physics" was cut to "physic" and matched as a word beginning, so "physics journals"
  found the New England Journal of Medicine through "physicians". The word as typed
  still matches the start of a word, and its singular now matches only that whole
  word: "journals" still finds "journal" and "clinics" finds "clinic".
- **Every region can be found in the Region filter.** It listed regions, groupings,
  and research networks together, ranked by service count, and dropped any with no
  services. So the first eight were the OECD, NATO, the G20, the G7, Five Eyes, APEC,
  the Americas, and Northern America. South America, Southern Asia, and Central Asia
  were behind "more", and Central America, the Caribbean, Middle Africa, Melanesia,
  Micronesia, and Polynesia were missing. Region is now the UN M49 tree, every region
  shown with its count even when that is 0, then the common names. Political
  groupings and research networks have their own Grouping filter. (#34)
- **"+ N more" in the filters works.** Every filter list showed all of its values from
  the start, because the list's style overrode `hidden`, so the button revealed
  nothing. Each list now shows eight and the button the rest. (#36)
- The "Requested attribute" filter lists every attribute services request (215
  today), not only the 40 most requested. (#36)
- The federations page links every policy and MRPS document a federation publishes,
  not just the first. DFN-AAI's and RedIRIS's separate SP and IdP terms were hidden. (#36)
- "West Asian", "Southern African", "Polynesian", and similar spellings now read as
  their regions, and "South Asia" says that M49's Southern Asia includes Iran. (#34)
- Ingest in AWS can save its sharing and sector caches. It had no S3 access to them,
  so every run asked Claude for the same 800 labels again and kept none, and most
  services never got a label. (#32)
- Ingest stops starting Claude requests and website lookups before Lambda's 15-minute
  limit, so it always saves its caches and writes the snapshot. A run that reached the
  limit lost all of its work and wrote nothing. The first run of 0.2.0 took 753 of the
  900 seconds. (#32)
- A push to `develop` no longer cancels an image build for a release tag or a manual
  promotion run; each event and ref builds on its own. (#29)

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

[Unreleased]: https://github.com/incommon/InCatalog/compare/v0.3.0...develop
[0.3.0]: https://github.com/incommon/InCatalog/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/incommon/InCatalog/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/incommon/InCatalog/tree/v0.1.0
