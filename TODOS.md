# TODOs

## A "Log in" button on each service's expanded view

Let someone go from a service's page straight to signing in to it, through their own
institution. Metadata gives three routes. Each works for only some SPs, so the button
has to say which route it uses and show only when one exists.

- **SP-initiated, through the SP's own login endpoint.** An `init:RequestInitiator`
  endpoint (Shibboleth's login handler) starts a login for any IdP, often with an
  `entityID` parameter for the IdP. Ingest notices it today
  (`supports_request_initiator` in `catalog-core/src/extract.rs`) but doesn't keep the
  `Location`. Store it, with its binding.
- **IdP-initiated, through the user's own IdP.** Once the user has picked their
  institution, link to that IdP's unsolicited-SSO endpoint for this SP. For Shibboleth
  that is `/idp/profile/SAML2/Unsolicited/SSO?providerId=<SP entityID>`. This needs the
  IdPs' `SingleSignOnService` endpoints and software, which ingest doesn't keep yet:
  it only counts IdPs. Many SPs reject unsolicited responses, so treat it as
  best-effort and say so.
- **Fallback: the service's own front door.** `mdui:InformationURL` or the
  organization URL, labelled as the site, not a login.

Questions to settle:
- **Choosing an institution.** Use SeamlessAccess (its widget, or its persistent
  choice) or a picker of our own. The choice stays in the browser only, never on our
  servers. Any third-party script or endpoint needs a deliberate change to the CSP.
- **Who it can work for.** Use the sharing label (#16). A *bilateral* SP should say
  "for <institution> users only" and offer no button to everyone else. A
  *multilateral* one still depends on the user's IdP releasing the attributes it needs.
  Say that on the button, as the About page does, rather than promising access.
- **No open redirect.** Build every URL only from metadata and the chosen IdP's
  metadata, never from query input. Allow only `https:`, and refuse private and
  loopback hosts, as the logo fetcher does.
- **Tests.** URL construction for each route, which button shows for each kind of SP,
  and that a crafted entityID or parameter can't send the browser anywhere else.

## Show why InCommon filtered eduGAIN SPs out of its import

The catalog already knows *which* eduGAIN SPs InCommon doesn't import: availability
`edugain-only` ("in eduGAIN but not in InCommon's filtered import", `Availability::EdugainOnly`
in `catalog-core/src/model.rs`). It doesn't know *why*. Add the reason from InCommon's
metadata filtering report.

- **Find and read the source first.** Locate the report InCommon publishes on its eduGAIN
  import filtering, and check from the report itself, not from memory, what it gives
  per entity. Record its URL, format and the date it was read, as the region data does.
  If it gives only rules and not per-entity results, apply the rules in `catalog-core`
  and label each reason as derived from the rules, not reported.
- **Ingest it.** Fetch the report beside the aggregates, store each filtered entity's
  reason(s) with the report's date, and say in the ingest report how many
  `edugain-only` SPs have a reason and how many don't. Keep "filtered by InCommon"
  distinct from "not in InCommon's import for some other reason", such as timing
  between the two aggregates' publication.
- **Their own category.** Show filtered SPs as a category of their own, "Filtered from
  InCommon's import", with a facet on the reason. Add an API filter and an OpenAPI
  entry, and NL phrasings ("services InCommon filtered out", "why isn't X in InCommon").
  The service page states the reason, with a link to the report and its date.
- **Red outline on their chips.** Outline these services' chips and cards in red. Define
  the red as a theme token that also works in dark mode. Don't rely on color alone:
  add a text label or icon and an accessible name, and explain the red in the floating
  key and on the About page.
- **Tests.** Parsing the report, joining it to entities by entityID, the category and
  facet counts, the chip styling class, and an entity with no matching reason.

## OFAC sanctions: countries in the Grouping filter, and entities

Let someone find the services tied to US sanctions: those in countries under OFAC's
country-wide programs, and those whose organization may be on OFAC's lists.

- **Reverse a written decision first.** The README (§ Regions) and `about` in
  `catalog-core/data/regions.json` both say sanctions lists are left out on purpose, as
  groupings that label countries by a judgment. Decide that OFAC's lists are in because
  the US government publishes them, not because we judge, and reword both places to say
  so. Every label says whose list it is: "US sanctions (OFAC)", never "sanctioned
  countries".
- **Find and read the source first.** OFAC publishes sanctions programs, not one list
  of countries, and most programs target named persons rather than whole countries.
  Read the programs from OFAC itself, not from memory, and list only the country-wide
  ones. Record the URL, the date it was read, `as_of` and, as programs are lifted,
  `left`. Some programs cover part of a country (regions of Ukraine) with no ISO code of
  their own: say in a `note` how that was handled, as the African Union's note does for
  Western Sahara.
- **Its own kind.** A sanctions program is not a treaty or an alliance, so give it a
  `RegionKind` of its own rather than `political`, shown in the Grouping filter under a
  heading of its own. Programs change faster than memberships: a year is too long
  between checks, so give this kind a shorter limit in the `checked` test.
- **Entities: the SDN and consolidated lists.** OFAC publishes them as files from its
  Sanctions List Service. Fetch them at ingest beside the aggregates, with their
  publication date, and compare entries against each provider's organization name and
  domains. Compare only entries for entities, never individuals. A name is not an
  identity, so the catalog never states that a provider is sanctioned. A name match is a
  "possible match" and shows the list entry it matched. A match is confirmed or
  dismissed only through the curation overlay, never automatically. Say in the ingest
  report how many providers matched.
- **Not a compliance check.** The About page and each label say that this is a pointer
  to OFAC's own lists, as of a date, and not sanctions screening or legal advice.
  Link OFAC's search tool for anyone who needs an answer.
- **A marginally menacing chip and card for OFAC entities.** Make it noticeable at a
  glance and a little ominous, without looking like an alarm. For example, a near-black
  fill with a thin amber hazard-stripe edge, and a small warning glyph. It has to stay
  distinct from the other markings, so no red (InCommon-filtered) and no motion (dead
  services' pulsing border). Make its colors theme tokens that work in dark mode.
  - **Only a confirmed match gets the full style.** A possible match is a name
    collision until curation confirms it, so it gets only a quiet mark (the glyph, no
    stripe or dark fill). Services merely in an OFAC country get neither.
  - **Not by color alone.** Add a text label ("On OFAC's SDN list, confirmed <date>"
    or "Possible OFAC match") and an accessible name, and explain both in the
    floating key (see below).
  - **Settle combinations.** Say how it sits with the dead border and the red outline
    when a chip has more than one.
- **API and NL.** Add the API filter and an OpenAPI entry, and NL phrasings ("services
  in OFAC countries", "sanctioned countries"), resolved to the OFAC grouping, so the
  word "sanctioned" never searches service text.
- **Tests.** Parsing both lists, a program that covers part of a country, the shorter
  `checked` limit, a name collision that stays a possible match, the curation overlay
  confirming and dismissing a match, that no individual's entry is ever matched, and
  the chip's classes for a confirmed match, a possible match and an OFAC-country
  service.

## A floating key to the chips' markings, in the lower right of the chip view

The service chips and cards are gaining markings: the red outline for services
InCommon filtered from its import, the alive indicator and the dead services' pulsing
white-black border, and the OFAC style. Each TODO says to explain its marking in a
legend. Make that one legend: a key floating in the lower-right corner of the chip
view (the `.grid` of `serviceCard`s in `web/app.js`, on the results page and on the
provider and federation pages).

- **What it lists.** One row per marking: a small sample swatch drawn with the same
  classes as the chips, so the two can't drift, a short label, and a link to the
  About page's longer explanation. List only the markings on the chips currently
  shown, and hide the key when there are none. A new marking adds its row as part of
  its own change.
- **Where it floats.** Pin it to the lower right of the chip view, not the window, so it
  never covers the facets. Leave room under the last row of chips so the key never
  hides one, nor the paging controls.
- **It can be collapsed.** It can collapse to a small "Key" button. Remember the choice
  in the browser (`localStorage`, wrapped in try/catch, defaulting to open when
  storage is unavailable). On narrow screens, start collapsed.
- **Accessible.** A labelled region with a heading, reachable by keyboard, with each
  swatch's meaning in text. Under `prefers-reduced-motion`, show the dead border's
  static version, as the chips do. Both themes.
- **Tests.** The key lists exactly the markings present on the page, hides when there
  are none, doesn't cover the last chip, keeps its collapsed state across reloads, and
  swatches use the chips' own classes.

## Rename the AWS resources from `catabasis` to `incatalog`

The code and docs use `incatalog`, but the deployed AWS resources still carry the old
working name. These must change together, as a planned migration:

- **`var.name` default** in `infra/terraform/variables.tf`. Every resource is named from
  it (ECR repo, both Lambdas and their log groups, the S3 data and site buckets, CloudFront,
  the EventBridge rule, API Gateway). Changing it makes `tofu apply` destroy and
  recreate them, so copy the data and site buckets' contents and push the image to the new
  ECR repo first (or `tofu state mv` / import where a resource can keep its identity).
- **The state bucket** `catabasis-tfstate-<account>` and key `catabasis/terraform.tfstate`
  in `infra/terraform/backend.hcl` and `infra/state-bucket.sh`. S3 buckets can't be
  renamed: create the new bucket, update both files, run
  `tofu init -backend-config=backend.hcl -migrate-state`, then delete the old bucket.
- **The GitHub `dev` environment's variables.** `ECR_REPOSITORY` names the ECR repository
  and `AWS_ROLE_ARN` the push role (`<name>-github-image-push`); both change with
  `var.name`, so update them from the new OpenTofu outputs.
- **The `catabasis-*` ARNs** in `infra/deploy-policy.json`. Update the policy in IAM
  wherever it is attached, before the first deploy under the new names, or deploys lose
  permission.

## Finish removing Terraform's name from the OpenTofu setup

OpenTofu has applied the stack since #35. Some references still say Terraform. Do the
first two in one PR into `develop`, and the third separately, later:

- **One stale comment.** `crates/catalog-claude/src/lib.rs` says "Terraform state". Under
  `crates/`, so the PR runs the Rust suite.
- **`infra/terraform/` → `infra/opentofu/`**, as hyperdrive did. About 20 references
  change: Makefile, README, TODOS, CONTRIBUTING, `ci.yml`, `build-images.yml`,
  `infra/state-bucket.sh`, `web/config.js` and `.dockerignore`. Nothing changes in AWS,
  since the backend settings stay the same, but every checkout runs `make init` again.
  The `web/config.js` edit starts an image build on `develop` (a push, no deploy).
- **The CI job `terraform fmt / validate` → `tofu fmt / validate`.** `main`'s branch
  protection requires the job by its exact name, so the timing matters. The rename
  reaches `develop` first, while `main` keeps the old job until the next release merges.
  Switch the protection while the next release PR into `main` is open, after its CI has
  reported the new name. Switched earlier, a hotfix PR into `main` would wait forever
  for a check that never reports; switched later, the release PR would. Then update
  CONTRIBUTING's CI table and its rule about this name.

Leave alone: `.terraform/`, `.terraform.lock.hcl`, the `terraform {}` block and the
`terraform_version` key that `make tofu-version` parses, which are OpenTofu's own names.
Also leave the state key `catabasis/terraform.tfstate` (move it only with the AWS rename
above, which moves the state anyway), past CHANGELOG entries, and the "never run
`terraform`" warnings.

## Label services alive or dead, and hide the dead ones by default

Metadata outlives the services it describes. Check each service's hosts and label each
service **alive** when it answers, or **dead** when it no longer does: a 404 or a 5xx,
DNS that fails or no longer points at the service, or TLS that doesn't validate.
Searches hide dead services unless asked for them. The logo fetcher
(`catalog-ingest/src/logos.rs`) already has most of what this needs: the `PublicOnly`
resolver that refuses private and cloud-metadata addresses, failure classification,
retries with backoff, the network-down guard, a cache with recheck intervals, and the
run's `Deadline`.

- **What to check, and how.** Check the service's own front door: `endpoint_hosts`
  (the `AssertionConsumerService` hosts), and `information_urls`, the organization URL
  and `privacy_statement_urls`. Judge an ACS host only by DNS, connection and TLS,
  never by its HTTP status. An ACS expects a SAML POST, so many answer a GET with 400,
  405 or 500 while working fine. The status code counts only for pages meant to be read
  (the information, organization and privacy URLs).
- **Each kind of failure, kept apart.** DNS: NXDOMAIN is evidence; SERVFAIL and timeouts
  are transient. HTTP: 404 and 410 are evidence; 5xx is transient until it persists; 403
  and 429 say nothing. TLS: record which check failed (expired, wrong host name,
  untrusted chain, self-signed). TLS on the ACS host matters most, because the browser
  posts the assertion there.
- **DNS that no longer points at the service.** This one has no single test, so record
  the signals and label it "may no longer be this service", never as fact:
  - the registered domain has lapsed, or now serves a registrar's parking or for-sale
    page
  - a CNAME points at a cloud resource that no longer exists (a deleted S3 bucket,
    Azure or Heroku app, or GitHub Pages site)
  - the site redirects to an unrelated domain
  - against the last good check, both the addresses and the certificate's subject and
    issuer changed at once

  Keep the parking-page and cloud-provider fingerprints in a data file, each with its
  source and a `checked` date, as `regions.json` does.
- **Dangling ACS hosts are a security finding, not a label.** Whoever claims a dangling
  CNAME on an ACS host can receive users' assertions. Don't publish those details in the
  snapshot or the site, which would hand out a takeover list. Show only "unreachable".
  Put the specifics in a report kept private for the federations' operators, and decide
  first who receives it and how.
- **One bad check is not a dead site.** Label a host only after it fails several checks
  in a row over several days. Settle the count and the span from the data. Record the
  first failed check and the last good one, and clear the label on the next success.
  Some sites answer only from campus networks (split DNS, IP allow-lists), so a failure
  seen from AWS is not proof. Say so on the About page. `dead` is the status's name in
  the code and the API, but the site's words are "unreachable from InCatalog since
  <date>", never "dead".
- **Be a polite client.** Send at most one request per host per check (HEAD, falling back
  to GET), with a user agent that names InCatalog and links the About page. Spread the
  rechecks over the schedule, as the logo cache does, rather than probing every host on
  every run. Keep the results in their own cache beside `logo-cache.json`.
- **Alive is a positive finding, not the absence of a failure.** Each service has one
  status, with its reasons and dates:
  - **alive:** on a recent check, an ACS host resolved, connected and passed TLS. A
    broken information, organization or privacy page doesn't make a service dead.
    List those pages as broken links on the service instead.
  - **dead:** unreachable past the threshold, or "may no longer be this service".
  - **unknown:** not checked yet, or only inconclusive answers (403, 429, timeouts,
    campus-only).

  Show an alive indicator on each alive service's chip and card, with a text label
  and an accessible name, not only a mark. An unknown service gets neither the alive
  indicator nor the dead border, and its card says "not checked yet" or "no clear
  answer yet". Say on the About page what alive means and how recent "recent" is.
- **Searches show alive and unknown services by default, and hide dead ones.** The site
  and the API filter out dead services unless asked otherwise: a "Show unreachable
  services" toggle on the site, and `status=` in the API, taking `alive`, `dead` and
  `unknown` (several at once, comma-separated) or `all`, defaulting to
  `alive,unknown`, with an OpenAPI entry. Facet counts and provider and federation
  counts follow the same filter, and the landing page says which services its totals
  count. NL phrasings ("dead services", "broken links", "services with bad
  certificates") turn the filter off by themselves, and the reading of the query says
  so. The API's default results change, so the CHANGELOG records it under Changed.
- **Nothing vanishes at rollout.** At first nearly everything is unknown, and unknown
  shows, so the default can ship before the checks have covered the catalog. Dead
  services drop out of default results only as checks find them, and a campus-only
  service that never answers stays unknown and stays visible. Run `make backfill` to
  check every service sooner.
- **Dead services get a pulsing white-black border on their chip and card.** Make the
  white and black theme tokens so the border reads against both themes' backgrounds,
  and keep `button.chip:hover` from overriding it. With `prefers-reduced-motion`, show
  a static alternating white-black border instead, as `network.js` already respects
  that setting. Don't rely on the border alone: add the text label "unreachable from
  InCatalog since <date>" and an accessible name, and explain the border in the
  floating key and on the About page. Settle how it sits with the red outline from the
  InCommon-filtered TODO when a chip has both.
- **Report it.** The ingest report counts services alive, dead and unknown, and each
  kind of failure.
- **Tests.** Local fixture servers for 404, 500, an expired certificate and a wrong-host
  certificate, and a stub resolver for NXDOMAIN and SERVFAIL. Also:
  - an ACS answering a GET with 500 still counts as alive
  - a broken information URL alone doesn't make a service dead
  - the failure threshold and recovery
  - each fingerprint
  - the default filter keeps alive and unknown services and drops dead ones, and each
    `status` value, in the API and in facet counts
  - NL turning the filter off
  - the chip's dead and alive classes, including under reduced motion
  - the private findings never reaching the public snapshot
