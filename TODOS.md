# TODOs

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

## Show the new search behaviors on the main page and document them in About

The landing page's "try:" row (`web/app.js`, near the search box) still offers the three
original examples. The About page (`aboutView()` in `web/app.js`) has Snapshot, Sources,
Read with care, Ingest warnings and Categories, but nothing on how to search.

- **Examples on the main page.** Add examples of each new behavior:
  - several federations at once (#9)
  - production vs test vs dev (#14), e.g. "test instances of Canvas"
  - regions and groupings (#15 and later), e.g. "R&S services in Europe", "SPs in the
    EU that request email"
  - multilateral vs bilateral (#16), e.g. "services anyone in eduGAIN can use"

  Keep the row short (rotate or pick a few) so it doesn't crowd the search box.
- **A "How to search" section in About,** for the public. Cover:
  - each filter and what it matches; for example, a set of federations ORs within the
    filter and ANDs with the others
  - the plain-language phrasings the search understands
  - the default view, if one is chosen for environments (#14 left it open)
  - what "multilateral" does and doesn't promise: open to any IdP in principle, access
    still depends on the IdP releasing attributes
  - how regions and groupings are defined: source, and the date each membership list
    was checked

  Say which labels are generated by Claude, as the summaries already do.
- **Keep it honest.** A test should run every example through the natural-language
  reading and check that it produces the intended filters and returns results on the
  fixture snapshot, so an example can't silently stop working.
