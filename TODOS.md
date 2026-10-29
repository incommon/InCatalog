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
