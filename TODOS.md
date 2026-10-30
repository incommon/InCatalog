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
  add a text label or icon and an accessible name, and explain the red in the legend
  or on the About page.
- **Tests.** Parsing the report, joining it to entities by entityID, the category and
  facet counts, the chip styling class, and an entity with no matching reason.

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
