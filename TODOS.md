# TODOs

## Classify SPs as bilateral or multilateral, and search on it

Much of the catalog is bilateral SPs: registered for one institution's users, like the
hundreds of 1:1 Zoom SPs published per campus. They are noise to someone looking for a
service they could use. A true multilateral SP accepts users from any IdP, given enough
attributes in the assertion. For example, the Internet2 IAM platform lets any user at
any IdP in eduGAIN in. Label each SP in ingest, and let people search on the label.

- **Gather deterministic signals first,** in `catalog-core`, and pass them to the
  classifier as facts:
  - Entity categories: R&S, CoCo, and REFEDS Anonymous, Pseudonymous and Personalized
    Access lean multilateral.
  - Export to eduGAIN (`Availability`).
  - A discovery-response or request-initiator endpoint, which shows the SP expects
    many IdPs.
  - An entityID, host or display name that carries one institution's name.
  - Clusters of many registrations that share a vendor host pattern and differ only by
    tenant, such as `<campus>.zoom.us`. These lean bilateral.
  - The registrant organization being the vendor rather than the institution.
- **Classify in ingest through Bedrock,** beside the summaries in
  `catalog-ingest/src/enrich.rs`:
  - Labels: `multilateral`, `bilateral` or `unknown`, plus a short reason and the
    signals it relied on.
  - Provenance: held apart from the metadata with the model and date, like
    `enrichment`, and labeled in the UI as generated.
  - Caching: cached by a hash of its inputs. Give it its own cache key or prompt
    version, so adding it does not rewrite every existing summary.
  - Cost: capped per run like `summary_max_per_run`.
  - Clusters: a whole tenant cluster can be classified once, not per entity.
- **Search.** A `sharing` filter and facet in `ServiceQuery`, the API and the OpenAPI
  document. NL phrasings such as "services anyone in eduGAIN can use" or "not
  institution-specific". Consider ranking bilateral entries below multilateral ones
  in mixed results, or collapsing a tenant cluster into one result ("Zoom — 300
  institution-specific registrations").
- **Say what it means.** "Multilateral" means open to users of any IdP in principle.
  Access still depends on the user's IdP releasing the attributes the SP needs. The
  badge and API docs should say so, not promise access.
- **Validate before shipping.** Build a hand-labeled sample covering per-campus Zoom,
  Canvas and Box tenants, publishers, the Internet2 IAM platform, and research
  collaboration SPs. Report precision for each label, and keep the sample as a
  regression test for prompt changes.

## Search by region: "the Americas", "Europe", "the EU", "Asia", "Oceania", "BRICS"

Today a search can name one country (`country=SE`, or "in Sweden" through the reading in
`catalog-core/src/nl.rs`), matched against the country codes of the federation that
registered the service. People also think in regions that span many registration
authorities: "Research & Scholarship services in Europe", "SPs registered in the
Americas", "anything from BRICS countries".

- **Regions as named sets of countries.** Define each region as a list of ISO 3166
  codes in `catalog-core`, with its source:
  - **Geographic regions** (the Americas, North/Latin America, Europe, Asia, Africa,
    Oceania, and subregions) from UN M49.
  - **Political groupings** (the EU, the EEA, BRICS, …) with the date of their
    membership list, since it changes. BRICS has grown well beyond its original five,
    and the EU is not "Europe".
- **Filter on a set of countries.** Make `country` accept several values (as the
  multi-select federation work does for `federation`) and add `region=`, which expands
  to its countries. Report the region and its expansion in the response, so a reader
  can see what "Europe" meant.
- **Multi-country federations.** Some federations list several country codes. Decide
  whether one is in a region when any of its countries is, and document the choice.
- **Natural-language search.** Teach the rules reading the region names and their
  common forms ("European", "in the EU", "Asia-Pacific", "LatAm"), and let the Claude
  reading in `catalog-api/src/nl.rs` produce a region rather than guessing a country
  list. An ambiguous name should say how it was read. For example, "America" alone
  more likely means the United States than the Americas.
- **UI.** Show the active region as one chip, expandable to its countries, and keep it
  in the URL state.
- **Tests.** The expansion, membership dates, the multi-country rule, and the NL
  phrasings, including false positives such as "European Molecular Biology Laboratory"
  as an organization name.

## Rename the AWS resources from `catabasis` to `incatalog`

The code and docs use `incatalog`, but the deployed AWS resources still carry the old
working name. These must change together, as a planned migration:

- **`var.name` default** in `infra/terraform/variables.tf`. Every resource is named from
  it (ECR repo, both Lambdas and their log groups, the S3 data and site buckets, CloudFront,
  the EventBridge rule, API Gateway). Changing it makes `terraform apply` destroy and
  recreate them, so copy the data and site buckets' contents and push the image to the new
  ECR repo first (or `terraform state mv` / import where a resource can keep its identity).
- **The state bucket** `catabasis-tfstate-<account>` and key `catabasis/terraform.tfstate`
  in `infra/terraform/backend.hcl` and `infra/state-bucket.sh`. S3 buckets can't be
  renamed: create the new bucket, update both files, run
  `terraform init -backend-config=backend.hcl -migrate-state`, then delete the old bucket.
- **The GitHub `dev` environment's variables.** `ECR_REPOSITORY` names the ECR repository
  and `AWS_ROLE_ARN` the push role (`<name>-github-image-push`); both change with
  `var.name`, so update them from the new Terraform outputs.
- **The `catabasis-*` ARNs** in `infra/deploy-policy.json`. Update the policy in IAM
  wherever it is attached, before the first deploy under the new names, or deploys lose
  permission.

## Find out why logos are still missing, and close the gaps

In the local snapshot of 2026-10-28, **792 of 8,633 services (9.2%) have no logo**.
5,576 use a working `mdui:Logo` and 2,265 a logo found on their organization's website.
`data/logo-cache.json` shows why the rest have none:

| Services | Reason |
|---:|---|
| 439 | Website **never looked up** yet. Each run looks up at most `CATALOG_LOGO_MAX_LOOKUPS` sites (default 150), busiest first, so the long tail waits. |
| 331 | Website lookup **failed**. Of the 571 failed sites, 526 were DNS failures, all within one 2.5-minute window (16:02–16:05 UTC). 27 of a sample of 30 of those hosts resolve now, so most were transient. Each failure is cached for 3 days. |
| 22 | The site loaded but had **no usable logo**. |

Of the 770 services in the first two groups, 280 have a `mdui:Logo` that is broken; the
other 490 have none at all. Broken metadata logos were 477 of the 1,882 distinct URLs.
That is the registrants' problem, but it is what pushes those services onto the website
path.

- **Check AWS too.** These figures are from a local run. Measure the deployed snapshot and
  its `logo-cache.json` in the data bucket the same way. Bedrock and the Lambda resolver
  may behave differently.
- **Stop transient DNS failures sticking.** Retry a DNS failure within the run, with
  backoff. Resolve with lower or adaptive concurrency (lookups run 24 sites at a time,
  `buffer_unordered(24)` in `catalog-ingest/src/logos.rs`). Cache transient failures for
  hours rather than 3 days, and keep them out of the per-run cap.
- **Work through the backlog faster.** Raise the cap, or retry recent errors first, so
  the 439 never-looked-up sites are reached in days rather than weeks.
- **Report it.** Have ingest log services without a logo by reason (never looked up,
  DNS, HTTP, no candidate, broken metadata logo), so the gap is visible on each run
  rather than reconstructed from the cache.
- **Look at the 155 inconclusive metadata logos** (403, 429, timeouts, 5xx). They are
  shown, but a browser may not load them either.
- **Consider a fallback** for what is left, such as the IdP or federation logo, or a
  clear placeholder, rather than nothing.

## Search by more groupings: "Schengen", "NATO", "OECD", research networks, …

Region search (`region=`, `catalog-core/src/regions.rs`) covers UN M49 geographic regions,
Asia-Pacific, the EU, the EEA, and BRICS. Extend it:

- **Further political, security and economic groupings,** each with its source and a
  dated membership list:
  - *Treaty and travel areas:* the Schengen Area, which is not the EU (it has non-EU
    members such as Norway and Switzerland, and EU states outside it), the eurozone, and
    the Nordic Council.
  - *Security alliances:* NATO, and Five Eyes.
  - *Economic and trade blocs:* the OECD, G7, G20, ASEAN, APEC, Mercosur, USMCA, the
    African Union, the Gulf Cooperation Council, and the Commonwealth.
  - *Research networks,* where people search that way: the countries of GÉANT's NRENs,
    and of the regional networks RedCLARA, APAN, WACREN, UbuntuNet and ASREN.
- **Keep the data maintainable.** Store groupings as data (say
  `crates/catalog-core/data/regions.json`), each with its source, the date its list was
  checked, and per-country join dates where they matter, rather than hard-coding them.
  Membership moves (Sweden and Finland joining NATO, Bulgaria and Romania joining
  Schengen), so a test should fail once a list is older than a year, as a prompt to
  recheck it.
- **Deliberately leave out** groupings whose membership is contested or politically
  sensitive to assert, and anything that labels countries by a judgment, such as
  sanctions lists. If one is added later, document how its list was chosen.
- **Natural-language search.** Teach the rules and the Claude reading the new names and
  their common forms ("Schengen countries", "NATO members", "OECD").
