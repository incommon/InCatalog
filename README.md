# InCatalog: a federation service catalog

A searchable catalog of the services (SAML service providers) you can reach through
**InCommon** and **eduGAIN**, built entirely from live, published federation metadata.

- **`catalog-ingest`** (Rust) fetches the InCommon and eduGAIN metadata aggregates plus
  eduGAIN's federation list, merges them, and writes one compressed JSON snapshot.
- **`catalog-api`** (Rust, Axum) serves that snapshot as a read-only JSON API with
  search, facets, providers, federations, and an OpenAPI document.
- **`web/`** is a static, build-free front end. It runs from any static host (S3 and
  CloudFront here) and calls the API. It uses InCommon's colors and typeface (Poppins). Its
  landing page is an animated globe of the federations in eduGAIN, each at its country
  and sized by the services it registers, with metadata pulses running between each
  federation and the eduGAIN hub.

Everything runs serverless on AWS: an EventBridge schedule triggers the ingest Lambda,
the API Lambda sits behind API Gateway, and CloudFront serves the site.

```
EventBridge (daily) ─▶ λ catalog-ingest ─▶ S3  snapshots/catalog.json.gz
                                                   ▲
browser ─▶ CloudFront ─▶ S3 web/                   │ (ETag checked every 5 min)
   └────▶ API Gateway (HTTP) ─▶ λ catalog-api ─────┘
```

## Quick start

```bash
make test        # unit + fixture tests; Claude and Bedrock are local stand-ins, no AWS
make test-image  # needs Docker: image contents, signatures both ways, ingest + serve
make dev         # ingests live metadata if no snapshot exists, then serves API + web on :8080
```

The first `make dev` downloads about 210 MiB of XML (InCommon and eduGAIN). Downloads are
cached in `data/cache/`, and later runs send conditional GETs, so an unchanged aggregate
costs one `304`. A full ingest takes about 10 seconds.

## Sources

Each source answers a different question. The catalog keeps them apart instead of
blending them.

| Source | URL | What it answers |
|---|---|---|
| InCommon metadata aggregate | `https://mdq.incommon.org/entities` | InCommon's registrations, plus InCommon's **filtered** import of eduGAIN: what an InCommon identity provider can see |
| eduGAIN metadata aggregate | `https://mds.edugain.org/edugain-v2.xml` | Everything every member federation exports to eduGAIN |
| eduGAIN database API | `https://technical.edugain.org/api.php?action=list_feds_full` | Federation names, countries, status, policies, and the registration authority that joins each federation to its entities |
| InCommon MDQ (per entity) | `https://mdq.incommon.org/entities/{sha1}…` | One entity's live metadata. Linked from every InCommon-visible service |
| REFEDS Metadata Explorer (MET) | `https://met.refeds.org` | Every federation that publishes a given entity. Linked, not ingested |
| REFEDS specifications | `https://refeds.org/specifications` | The entity-category and assurance URIs, encoded in `catalog-core/src/categories.rs` |

The retired `md.incommon.org` aggregate and `wayf.incommonfederation.org` discovery
service are not used. Every metadata resource now lives under `mdq.incommon.org`.

### What "available" means

Each service gets an **availability**, derived from who registered it and which
aggregates carry it:

| Key | Meaning |
|---|---|
| `incommon-only` | Registered by InCommon, not exported to eduGAIN |
| `incommon-and-edugain` | Registered by InCommon and exported to eduGAIN |
| `edugain-imported-by-incommon` | Registered elsewhere; included in InCommon's eduGAIN import |
| `edugain-only` | Registered elsewhere; in eduGAIN but **not** in InCommon's filtered import |
| `incommon-aggregate-only` | Registered elsewhere; in InCommon's file but not eduGAIN's |

The boundary is `mdrpi:RegistrationInfo/@registrationAuthority`. More than half of
InCommon's aggregate is other federations' registrations, so anything that ignores the
registrar double-counts "InCommon services."

### Traps the code handles

These are all cases where a naive implementation produces a clean, plausible, wrong
number.

- **EntityID clashes.** Some vendors' entityIDs are registered by InCommon *and* another
  federation (for example, the UK federation or DFN-AAI). eduGAIN carries only one
  registration, so the same entity looks like a UK entity in eduGAIN's file and an
  InCommon entity in InCommon's. The merge keeps InCommon's registration when InCommon
  has one, records the other registrar in `also_registered_by`, and adds a note. The
  snapshot of 2026-10-27 had **37** of these. Without this rule, InCommon's service count
  comes out 37 low.
- **Category synonyms.** Research & Scholarship has a REFEDS URI and a deprecated
  InCommon URI, plus http/https variants. Code of Conduct v2 exists under refeds.org and
  geant.net. Categories are sets of synonyms, matched on whole values only, because
  `…/sirtfi` is a prefix of `…/sirtfi2`.
- **The REFEDS security contact** is `contactType="other"` plus
  `remd:contactType="http://refeds.org/metadata/contactType/security"`. Checking only
  `@contactType` finds none anywhere.
- **Namespaces.** Every lookup is namespace-qualified. An unqualified lookup silently
  matches nothing, and nothing reads as zero.
- **Expired aggregates.** Ingest refuses to publish when an aggregate's `validUntil` has
  passed, unless you pass `--allow-expired`.

### Cross-check

The snapshot of 2026-10-27 (InCommon aggregate `Last-Modified: Tue, 27 Oct 2026 15:49:33
GMT`) was compared with an independent parser:

| Measure | Catalog | Reference |
|---|---|---|
| InCommon aggregate entities | 15,211 | 15,211 |
| InCommon-registered service providers | 6,471 | 6,471 |
| InCommon R&S service providers (all URIs) | 211 | 211 |
| eduGAIN aggregate entities / SPs | 10,773 / 4,137 | 10,773 / 4,137 |

Identity-provider counts are tallied per entity. An entity with both roles is counted as
a service, so eduGAIN's IdP figure here is slightly lower than a per-role count.

## API

Every endpoint is a `GET`. The OpenAPI document is at `/openapi.json`.

| Path | Purpose |
|---|---|
| `/v1/ask?q=` | Read a plain-language search and find results, loosening it when it matches nothing (see Natural-language search) |
| `/v1/interpret?q=` | Only the reading: the filters a plain-language search maps to |
| `/v1/meta` | Snapshot provenance (URLs, `Last-Modified`, SHA-256, `validUntil`, signature status), counts, caveats |
| `/v1/services` | Search. `q`, `federation`, `country`, `category` (comma-separated, AND), `availability`, `attribute`, `provider`, `source`, `provider_type`, `lang`, `sort`, `page`, `per_page`. Returns facet counts |
| `/v1/services/{id}` | One service. `id` is the lowercase hex SHA-1 of the entityID, which is the same identifier MDQ uses, so `{sha1}` is accepted as a prefix |
| `/v1/lookup?entity_id=` | One service, by entityID |
| `/v1/providers`, `/v1/providers/{id}` | Services grouped by registered organization name |
| `/v1/federations` | eduGAIN's federations joined to catalog counts, plus any registrars eduGAIN does not list |
| `/v1/categories` | Known categories with synonyms and counts, plus every raw tag value seen |
| `/v1/attributes` | Requested attributes, most requested first |

Facet counts for each dimension apply every *other* active filter, so a user can switch
between values without losing sight of the alternatives.

## Natural-language search

The search bar accepts plain language. `GET /v1/interpret?q=…` reads a query into the
same filters `/v1/services` takes, and the front end shows the reading above the
results so the user can check it or search the literal words instead.

| Query | Read as |
|---|---|
| R&S services in Sweden that request email | `category=research-and-scholarship`, `country=SE`, `attribute=mail` |
| UK federation services that need ORCID | `federation=UK-FEDERATION`, `attribute=eduPersonOrcid` |
| German library services | `country=DE`, words `library` |
| journals not in InCommon | `availability=edugain-only`, words `journals` |
| email marketing | words only (no "request" verb, so *email* is a subject, not an attribute) |

Two readers do this work:

- **Built-in rules** (`catalog-core/src/nl.rs`) are always on. They are deterministic,
  free, and need no network. Their vocabulary comes from the loaded snapshot: federation
  names and codes, countries and nationalities, category synonyms, availability
  phrasings, and attribute names with everyday aliases ("email", "ORCID", "last name").
- **Claude** (`catalog-api/src/nl.rs`) is optional. It reads queries of three or more
  words when an Anthropic key is configured. Rust has no official Anthropic SDK, so it
  calls the Messages API over HTTP. Its answer is a structured-output JSON schema whose
  enums are the catalog's own federations, countries, categories, and attributes, so it
  can only pick filters that exist, and the server validates the answer against the
  vocabulary again. The system prompt (that vocabulary) is prompt-cached, readings are
  cached in memory per snapshot, and refusals use the API's server-side fallback. Any
  error, timeout, or refusal falls back to the rules, and the reading says so.

Configure Claude with `ANTHROPIC_API_KEY` locally, or with
`anthropic_api_key_ssm_parameter` in Terraform (an SSM SecureString you create
yourself, so the key never enters Terraform state). The model defaults to
`claude-opus-5-5` at `low` effort. Set `CATALOG_NL_MODEL` / `nl_model` to trade quality
for cost and latency, and `CATALOG_NL_ENGINE=rules` to turn Claude off.

The search bar calls `GET /v1/ask?q=…`, which reads the query and then makes sure it
finds something. When the literal reading matches nothing it loosens one step at a
time, and returns a sentence saying what it loosened:

1. every filter and every word, then most of the words, then any of them
2. drop one filter at a time, softest first (requested attribute, country, seen-in,
   federation, provider type, availability, category)
3. the words alone, then the filters alone

"journals not in InCommon" has no literal match (no journal is eduGAIN-only), so it
shows journals across the catalog and says the eduGAIN-only filter was dropped. A query
nothing in metadata mentions ("adobe") still returns nothing, and the page says that
plainly.

Matching works on word beginnings: "journal" finds "journals", but "art" does not find
"start". Hyphens inside words are ignored ("e-book" = "ebook"). Rare words outweigh
common ones (in "survey tools", "survey" decides), and an exact phrase ranks first.
Short federation codes that are also ordinary words (LEAF, RASH, SIR, GIF, CAFe) only
count as federations when typed in capitals or next to "federation".

`/v1/services` takes `match=all` (default), `any` (at least half the words), or `some`
(at least one). Federation metadata does not record whether
a provider is commercial, so "commercial" or "vendor" only filters anything once a
curation overlay supplies provider types. Until then the reading says the word was
ignored.

## Logos

Each service shows its own `mdui:Logo` when that logo actually works, and otherwise
a logo from its organization's website.

1. **Metadata logos are checked.** Ingest fetches every `https:` `mdui:Logo` URL. One
   that returns a 404 or 410, points at a dead host, or serves an HTML error page instead
   of an image is marked `broken` and never shown. On 2026-10-27 that was **477 of the
   1,882** distinct logo URLs in the two aggregates. Inconclusive answers (403, 429,
   timeouts, 5xx) are left alone, since a browser may still load them.
2. **Missing or broken logos come from the website.** For a service with no working
   metadata logo, ingest fetches its organization's homepage (OrganizationURL, else
   InformationURL) and lists the candidate images: icon links, Apple touch icons,
   `og:image`, schema.org `logo`, and `<img>` elements marked as logos. Claude picks the
   organization's logo from that numbered list, and ingest confirms the pick is an image
   of at least 32 px. Claude never browses, so a hostile page can at most make it pick a
   wrong candidate, which the image check still screens.
3. **Provenance stays visible.** A website logo is stored in `service.discovered_logo`
   (URL, page, model, date), separate from the metadata. The API reports
   `logo_source: metadata | website`, and the service page says where the logo came
   from.

Every URL involved was written by a registrant, so the fetcher refuses private,
loopback, link-local (including `169.254.169.254`), and carrier-grade-NAT addresses,
non-default ports, and redirects to any of them. The checks happen at DNS resolution
and on every redirect.

Results are cached in `logo-cache.json` beside the snapshot, so later runs repeat
little:

| Entry | Rechecked after |
|---|---|
| Working logo URL | 14 days |
| Broken logo URL | 7 days |
| Inconclusive logo URL | 3 days |
| Website with a logo found | 30 days |
| Website with no usable logo | 14 days |
| Website that failed to load | 3 days |

Each run looks up at most `CATALOG_LOGO_MAX_LOOKUPS` websites (default 150),
starting with the sites behind the most services, so the first few runs fill the cache
gradually. Website lookup needs an Anthropic key, the same one natural-language search
uses (`ANTHROPIC_API_KEY`, or `anthropic_api_key_ssm_parameter` in Terraform).
Without one, ingest still checks metadata logos and reports how many services are
waiting. `CATALOG_LOGO_MODEL` (default `claude-opus-5-5` at `low` effort) chooses the
model, and `--skip-logos` turns all of this off.

## Generated summaries and keywords

Much of the metadata is thin: a display name, often no description, and none of the
words people actually search for ("lms", "survey", "e-journals"). During ingest,
Claude reads each entry's metadata and writes:

- **`summary`**: one to three plain sentences on what the service is and who offers
  it. For example: "Clemson University's instance of Canvas, a learning management
  system."
- **`keywords`**: 5 to 12 search terms, including synonyms and the kind of service.
- **`environment`**: `production`, `test`, `staging`, `development`, or `unknown`.
- **`basis`**: `metadata`, `metadata_and_general_knowledge` (also used widely known
  facts about a product the metadata names), or `insufficient`. With `insufficient`
  the summary is left empty rather than guessed.

This is generated text, so it is held apart from the metadata in `service.enrichment`,
with the model and date. The service page shows it in its own panel labeled as
written by Claude, with what it was based on. A card uses the summary only when the
metadata has no description, and marks it ✦. Registrants wrote the metadata, so it
reaches Claude marked as data to describe, never as instructions.

Search uses the summaries and keywords, so "lms" finds Canvas and Moodle instances
that never say "LMS". Entries marked test, staging, or development rank below the
live services they mirror.

**How it runs.** Each entry is keyed by a hash of exactly what Claude sees, so it is
written once and rewritten only when its metadata, or the prompt, changes. When more
than 25 entries need writing, ingest submits them to the Message Batches API (half
price) and collects the results on its next run, which is usually within the hour and
always within 24. One batch is open at a time. With 25 or fewer, the usual daily trickle
of metadata changes, ingest writes them immediately. A summary whose metadata has
changed is withheld until it has been rewritten. Everything is kept in
`summary-cache.json` beside the snapshot.

If a Batches API call fails, ingest still writes the snapshot, with the error in its
`warnings`. If a submit failed but a batch appears in the Console anyway (the request
arrived and the reply was lost), collect it instead of paying for it twice:

```bash
CATALOG_ENRICH_ADOPT_BATCH=msgbatch_… make ingest
```

The cache records which batch it collected, so leaving the setting on does nothing.

| Setting | Default | |
|---|---|---|
| `CATALOG_ENRICH_MODE` | `auto` | `batch`, `sync`, or `off` |
| `CATALOG_ENRICH_MAX` | 10000 | most entries sent per run |
| `CATALOG_ENRICH_MODEL` | `claude-opus-5-5` | at `CATALOG_ENRICH_EFFORT`, default `low` |
| `CATALOG_ENRICH_ADOPT_BATCH` | | a batch ID to collect that the cache has no record of |

To read a sample before generating everything:

```bash
CATALOG_ENRICH_MODE=sync CATALOG_ENRICH_MAX=20 make ingest
```

## Curation overlay

Metadata cannot say whether a provider is commercial, give a plain-language summary, or
add subject tags. `curation/curation.example.json` shows an overlay keyed by entityID or
by organization name. Copy it to `curation/curation.json` and `make ingest` picks it up.
In AWS, set `curation_file` in Terraform. Curated values are stored apart from metadata
(`service.curated`) and are labeled as curated in the UI, so a reader always knows which
statements came from the federation.

This is also the seam for provider-maintained content later: an editing workflow only
has to produce this file.

## Deploy (AWS)

Both Lambda functions run one arm64 container image (the `Dockerfile`), pushed to ECR.
The same image runs the site and API locally with `make run-image`.

```bash
make state-bucket    # once per account: S3 bucket for Terraform state, writes backend.hcl
make deploy          # builds and pushes the image, then terraform apply in infra/terraform
make seed            # once: copy local summary and logo caches up, so nothing is regenerated
make first-ingest    # the API needs one snapshot before its first cold start
```

`AWS_PROFILE` (default `incommon-hyperdrive-dev`) and `AWS_REGION` (default
`us-east-2`) pick the account. The deploying identity needs the permissions in
`infra/deploy-policy.json` on top of ECR, IAM, and CloudWatch Logs.

In AWS, every Claude task (reading searches, choosing logos, writing summaries) uses
Claude in Amazon Bedrock through `InvokeModel` (`CATALOG_CLAUDE_PROVIDER=bedrock-runtime`,
inference profile `us.anthropic.claude-haiku-5-5` by default), signed with each
function's IAM role, so there is no API key to store. The account needs Bedrock access
to that model. (Haiku 5.5 is not yet on Bedrock's Messages-API endpoint in these
regions; `CATALOG_CLAUDE_PROVIDER=bedrock` uses that endpoint for models that are.) Bedrock
has no Message Batches API, so summaries are written during each run, at most
`summary_max_per_run` (default 500) at a time.

The image includes `xmlsec1` and the two federations' signing certificates
(`certs/`), so ingest in AWS verifies both aggregates' signatures and refuses to
publish if either fails.

Outputs include `site_url` and `api_url`. Terraform writes `config.js` with the API URL
into the site bucket. The site is served with a strict Content Security Policy, because
logos and descriptions come from thousands of registrants. The front end also never
inserts metadata as HTML and only follows `http(s)`/`mailto` links.

Locally, signature verification is optional: install `xmlsec1` and pass
`--incommon-cert certs/incommon-mdq.pem --edugain-cert certs/edugain-mds-v2.pem` (or
`CATALOG_INCOMMON_CERT` / `CATALOG_EDUGAIN_CERT`). Without it, `signature_verified` is
`false`, the API says so in its caveats, and TLS from the operators' own hosts is the
integrity guarantee. **Do not use this catalog as a trust source for SAML configuration.**

## Limits

- **Configuration, not suitability.** Presence in metadata does not mean a service is
  contracted, appropriate, or currently running.
- **Free-text provider names.** Provider grouping uses the organization name each
  registrant chose. It does not resolve parent/subsidiary or system/campus relationships.
  Vendors that register one SP per customer appear as a single provider with many
  services, which is usually what a catalog reader wants.
- **SAML only.** Services that federate only with OpenID Connect are absent.
- **Assertions, not behavior.** A category tag records what was asserted or assigned in
  metadata. It does not guarantee that any particular identity provider releases
  attributes to the service.
- **Snapshot freshness.** The catalog is only as current as the last ingest (daily at 07:20 UTC
  by default). The MDQ link on each service shows the live record.

## Layout

```
crates/catalog-core     parser, model, categories, merge, search (no I/O)
crates/catalog-store    snapshot read/write: path, s3://, https://
crates/catalog-ingest   fetch + build snapshot (CLI and scheduled Lambda)
crates/catalog-api      Axum API (local server and Lambda)
web/                    static front end; network.js is the landing animation
infra/terraform         S3, Lambda, API Gateway, EventBridge, CloudFront
curation/               overlay example
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE). The bundled Poppins fonts in
`web/fonts/` are under the SIL Open Font License (`web/fonts/OFL.txt`).
