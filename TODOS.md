# TODOs

## The access button: remember the institution, and IdP-initiated logins

The button ships (`catalog-core/src/login.rs`, `loginEl` in `web/app.js`). It opens
InCommon Wayfinder on the service's behalf, else the SP's own `init:RequestInitiator`,
else its site. What's left:

- **Which routes really log people in.** Wayfinder hands the institution to the SP's
  discovery response endpoint with no state from the SP. Shibboleth's
  `/Shibboleth.sso/Login` starts a login from that, but other software (Canvas,
  SimpleSAMLphp) may not. Try a sample of each kind with a test IdP, and if one kind
  fails, send it to its RequestInitiator or its site instead.
- **The remembered institution on the button.** SeamlessAccess's dynamic button shows
  the user's last institution, from its persistence service. That needs its script or
  iframe, so a deliberate CSP change (`script-src`, `frame-src`), and SeamlessAccess
  then sees each service page view. Decide whether that's worth it.
- **IdP-initiated, through the user's own IdP.** For SPs with neither endpoint, link
  to the chosen IdP's unsolicited-SSO endpoint for this SP. For Shibboleth that is
  `/idp/profile/SAML2/Unsolicited/SSO?providerId=<SP entityID>`. This needs the
  user's institution, which only Wayfinder knows, so InCatalog would have to be a
  registered SP with its own discovery response endpoint. It also needs the IdPs'
  `SingleSignOnService` endpoints and software, which ingest doesn't keep: it only
  counts IdPs. Many SPs reject unsolicited responses, so treat it as best-effort.

## Rename the AWS resources from `catabasis` to `incatalog`

The code and docs use `incatalog`, but the deployed AWS resources still carry the old
working name. These must change together, as a planned migration:

- **`var.name` default** in `infra/opentofu/variables.tf`. Every resource is named from
  it (ECR repo, both Lambdas and their log groups, the S3 data and site buckets, CloudFront,
  the EventBridge rule, API Gateway). Changing it makes `tofu apply` destroy and
  recreate them, so copy the data and site buckets' contents and push the image to the new
  ECR repo first (or `tofu state mv` / import where a resource can keep its identity).
- **The state bucket** `catabasis-tfstate-<account>` and key `catabasis/terraform.tfstate`
  in `infra/opentofu/backend.hcl` and `infra/state-bucket.sh`. S3 buckets can't be
  renamed: create the new bucket, update both files, run
  `tofu init -backend-config=backend.hcl -migrate-state`, then delete the old bucket.
- **The GitHub `dev` environment's variables.** `ECR_REPOSITORY` names the ECR repository
  and `AWS_ROLE_ARN` the push role (`<name>-github-image-push`); both change with
  `var.name`, so update them from the new OpenTofu outputs.
- **The `catabasis-*` ARNs** in `infra/deploy-policy.json`. Update the policy in IAM
  wherever it is attached, before the first deploy under the new names, or deploys lose
  permission.

## Finish removing Terraform's name from the OpenTofu setup

OpenTofu has applied the stack since #35, and its directory is `infra/opentofu/`. One
reference still says Terraform, and it waits for the next release:

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

## Finish the alive/dead checks: names that point elsewhere, and the private report

Ingest checks login hosts and pages and labels services `alive`, `dead` or `unknown`
(README § Whether a service still answers, `catalog-core/src/health.rs`,
`catalog-ingest/src/health.rs`). What's left:

- **Names that no longer point at the service.** Today a lapsed domain whose name
  still resolves, or a CNAME to a deleted cloud resource, can look alive, because any
  answer counts. Add these signals, and label them "may no longer be this service",
  never as fact:
  - the registered domain has lapsed, or now serves a registrar's parking or for-sale
    page
  - a CNAME points at a cloud resource that no longer exists (a deleted S3 bucket,
    Azure or Heroku app, or GitHub Pages site)
  - against the last good check, both the addresses and the certificate's subject and
    issuer changed at once

  CNAMEs need DNS record lookups, which the system resolver doesn't give, so this
  needs a resolver crate (such as `hickory-resolver`) through `deny.toml`'s license
  review. Keep the parking-page and cloud-provider fingerprints in a data file, each
  with its source and a `checked` date, as `regions.json` does.
- **Dangling login hosts are a security finding, not a label.** Whoever claims a
  dangling CNAME on a login host can receive users' assertions. Don't publish the
  details in the snapshot or on the site, which would hand out a takeover list; the
  site shows only "unreachable". Put the specifics in a report kept private for the
  federations' operators, and decide first who receives it and how.
- **Provider and federation counts.** Search facets already leave dead services out by
  default, but the provider and federation listings still count every service.
  Decide whether they should follow the same default.
- **Tests with real TLS failures.** Failures are classified from the error's text,
  tested against messages rustls and the system resolver really give. Add local
  fixture servers with an expired and a wrong-host certificate, and a stub resolver
  for NXDOMAIN and SERVFAIL, so a change in those messages fails a test.

## WCAG 2.2 AA as a standing CI check, with a VPAT kept current

Make WCAG 2.2 Level AA something CI checks on every change to the site, not a one-off
audit. Keep the result as an Accessibility Conformance Report (ACR) on ITI's VPAT
template (the WCAG edition), in the repository and linked from the About page. Nothing
checks accessibility today, and `web/` has no build or Node tooling.

- **Automated tools find only some failures.** Most criteria need a person: whether
  alt text is meaningful, whether focus order makes sense, how a screen reader reads a
  card. So the check has two halves. An automated scan fails CI on what tools can
  detect. The ACR records each criterion's conformance level, how it was tested
  (automated rule IDs or a manual procedure), and the date. Check axe's own figure for
  how much it catches before quoting one.
- **The automated scan.** Use axe-core with its WCAG 2.2 AA tags, through Playwright,
  against `catalog-api` serving a fixture snapshot, so the run never fetches live
  metadata and gives the same answer every time.
  - Pin the Node tooling in its own directory (for example `tests/a11y/`), with a
    lockfile. It never ships in the image.
  - Add it to Dependabot, and give its licenses the same review `deny.toml` gives
    crates.
  - The org allows only GitHub-owned and verified-creator Actions, so install with
    `actions/setup-node` and `npm ci`, not a third-party axe Action.
- **What it scans.** Scan each page and the states that matter, in light and dark mode,
  at desktop width and at 320 CSS px (reflow, 1.4.10):
  - the landing globe
  - results with facets expanded and "+ N more" opened
  - a service page, a provider page and a federation page
  - the About page
  - the chip markings: the red outline, alive and dead, and OFAC
  - the floating key, open and collapsed
  - the floater at the bottom of every page (InCommon's lockup and the footer)

  Add the floating key's own tests (`chipView` in `web/app.js`): it lists exactly the
  markings on the page, is absent when there are none, rests below the last chip,
  keeps its collapsed state across reloads, and its swatches use the chips' classes.

  Add keyboard-only Playwright tests that tab through each page. They check that every
  control can be reached and operated, and that neither the floating key nor the
  floater ever covers the focused element (2.4.11, Focus Not Obscured).
- **Criteria the planned work runs into.** Settle these in the TODOs that add them,
  not after:
  - **Pause, Stop, Hide (2.2.2).** The dead services' zebra ring (which stops within five seconds) and the landing
    globe both move. Honoring `prefers-reduced-motion` may not be enough on its own,
    so give the page a control that stops animation, for example in the floating key.
  - **Three flashes (2.3.1).** Keep the pulse well under the flash thresholds.
  - **Target size (2.5.8).** Chips, facet buttons and "+ N more" need a 24×24 CSS px
    target, or the spacing that exception allows.
  - **Contrast (1.4.3, 1.4.11).** Check every marking's colors as a pair in both
    themes: the red outline, the white-black border and the amber stripe.
- **It reports on every PR, so it can be required.** Follow the `rust-gate` pattern: a
  change-detection step decides whether `web/`, the fixture or the scan changed, and a
  gate job with a fixed name always reports. Never use a workflow-level `paths:`
  filter. Pushes to `develop` and `main` always run it, and a weekly scheduled run also
  scans the deployed site, as the security audit does. Once it is stable, add it to
  `main`'s branch protection, and update CONTRIBUTING's CI table and its rules.
- **Ratchet, don't block on day one.** The first run will find existing failures. Fix
  them, or list each in a known-failures file with the rule, the page, a reason and a
  re-review date, the way `deny.toml` ignores advisories. A new failure fails CI, and
  so does a listed one past its date. The list only shrinks, like the coverage gate.
- **The ACR stays current.** Each criterion's entry carries the date it was last
  checked, and a test fails once any is more than a year old, as `regions.json`'s do.
  A criterion marked "Supports" while the scan reports a failure under it fails CI.
  Manual checks follow a written procedure in the repository: keyboard only,
  VoiceOver and NVDA, and 200% and 400% zoom. The ACR's date and version match the
  release it describes.
- **`make a11y`** runs the scan locally, like `make test`.
- **Tests of the check itself.** A deliberately broken fixture page (an image with no
  alt text, low-contrast text, a focus trap) must fail the scan, so a misconfigured
  run can't pass by scanning nothing. Also test that the gate fails when change
  detection fails, as `rust-gate` does.

## Logos: what measuring them left

Ingest measures each logo's artwork and the site crops, centers and fits it
(`catalog-ingest/src/logo_box.rs`, `logoPlacement` in `web/app.js`). What's left:

- **Logos that are invisible on the tile.** A logo drawn all in white for a dark page,
  such as Stanford GSB's, measures as all background. It is still shown, as a blank
  white tile. Show the monogram instead, and for a service whose only logo is blank,
  try the website lookup.
- **Front-end tests.** Test `logoPlacement` against each fixture's box (margins, a
  padded `viewBox`, `xMinYMid`, a wide wordmark, a mark beside words, a full-bleed
  square, no box) once the Playwright setup from the WCAG TODO exists.
- **SVGs with text** aren't measured, since text can't be drawn without fonts. Bundle
  one open font, or measure the rest of the drawing and widen the box by each text
  element's bounding box.

## The InCommon lockup: official SVGs and the brand guidelines

The lockup floats at the bottom of every page (`.floater` in `web/index.html` and
`web/styles.css`), from two PNGs whose original source isn't recorded
(`web/brand/README.md`). What's left:

- **Official files.** Get the lockup from Internet2's brand or marketing team, as
  SVG, in every version the guidelines allow, with the guidelines themselves. Replace
  the PNGs, and record each file's source, version, date and SHA-256 in
  `web/brand/README.md`. If a single-color version is allowed, one inline SVG in
  `currentColor` is simpler than swapping two images.
- **Written permission.** InCommon's Trademark and Logos page requires InCommon's
  written permission to use its logo. Keep a record of it, since `NOTICE` says
  "used with permission".
- **The guidelines' clear space and minimum size.** The site's 64px height and 16px
  clear space are its own choice, since nothing published gives either. Replace them
  with the guidelines' values.
- **The lockup's size once the page scrolls.** Scrolled, the lockup shrinks to 32px
  inside the footer's bar, white on navy in either theme, so the floater is no taller
  than the bar (`.floater.compact`). That puts "Powered by" well below the 63px
  incommon.org shows it at. Check 32px, and white on the bar's navy, against the
  guidelines' minimum size and color rules, or keep "Powered by" off the compact bar
  if they allow the InCommon logotype alone.

## TLS letter grades from testssl.sh, and a post-quantum badge

Scan each service's login hosts with [testssl.sh](https://testssl.sh) as a step of
ingest. Show each service's letter grade prominently on its chip and card and on its
details page. Give services whose hosts negotiate a post-quantum key exchange a badge.

- **What to scan.** Scan the `endpoint_hosts` (the `AssertionConsumerService` hosts),
  where browsers post users' assertions, on the port each ACS URL names. A service
  with several hosts shows its worst grade, and its details list each host's grade.
- **testssl.sh's own rating.** Use the grade testssl.sh computes (its JSON output's
  overall grade, following SSL Labs' rating guide), not a grading of our own. Read
  from testssl.sh's documentation, not from memory, which letters it gives (A+ to F,
  and its marks for trust and name-mismatch failures) and how it reports what capped
  a grade. Store the grade, the capping reasons, the scan date and the testssl.sh
  version.
- **Post-quantum.** The badge means the host negotiated a hybrid post-quantum key
  exchange in TLS 1.3 (an ML-KEM group such as `X25519MLKEM768`). It does not mean
  post-quantum certificates, which the public web PKI doesn't issue yet. Say exactly
  that on the badge's tooltip and the About page.
  - Check from testssl.sh's release notes which version detects these groups.
  - Check too which OpenSSL it needs to offer them. ML-KEM arrived in OpenSSL 3.5, so
    check what Amazon Linux 2023 ships.
  - If testssl.sh can't detect them, test with a client that offers only the hybrid
    groups and record whether the handshake succeeds.
- **In the image.** Install a pinned testssl.sh release, verified by checksum, plus
  what it needs (bash, `dig`, procps, and the OpenSSL that does the post-quantum
  test). Install them on the `amazonlinux:2023-minimal` runtime stage of the
  `Dockerfile`.
  - The bundled OpenSSL binaries testssl.sh ships are x86-64 only, and the image is
    arm64, so it uses the system OpenSSL. Check which legacy protocols and ciphers that
    can still test, and whether a grade can differ for that reason. Say so on the About
    page if it can.
  - testssl.sh is GPL-2.0, outside the permissive list `deny.toml` enforces for crates.
    It runs as a separate program, but the image distributes it, so decide that with
    the license policy's owners. Ship its license and source location in the image,
    and add it to `NOTICE`.
  - The image smoke test (`infra/test-image.sh`) checks the pinned version runs.
- **An ingest step with a budget.** A full scan takes minutes per host, and there are
  several thousand hosts (about 7,700 distinct entityID hosts today). So scan a capped
  number of hosts per run, a few at once, and stop at the run's `Deadline`. Cache
  results beside `logo-cache.json` and rescan each host on a schedule, about monthly.
  Measure the time per host and set the cap from it. `make backfill` works through the
  first pass.
- **Safe and polite scanning.** testssl.sh opens many connections, which can set off a
  campus's intrusion detection.
  - Resolve each host through the same public-only checks as the logo fetcher, and pass
    testssl.sh the checked address (`--ip`), so it never reaches a private network.
  - Scan one host at a time per address, never two at once.
  - Publish what InCatalog scans, how often and from where on the About page, with a
    contact and an opt-out. Keep opted-out hosts in a list that ingest skips.
  - Agree all of this with InCommon operations and security before the first scan.
- **Show it.**
  - **Chips and cards:** the grade as a letter badge, colored by grade with theme
    tokens. The letter itself is the text, so the color is never the only cue. Add the
    post-quantum badge beside it, with a text label and an accessible name.
  - **Details page:** the grade per host, what capped it, the scan date and the
    testssl.sh version, and a link to how grades are computed.
  - **No grade yet:** say "not scanned yet". Unreachable hosts are the alive/dead TODO's
    job and get no grade.
  - Add both badges to the floating key, a facet on grade and one on post-quantum, the
    API filters and OpenAPI entries, and NL phrasings ("services with an A grade",
    "post-quantum TLS").
- **Decide what's public.** A low grade is already visible to anyone who runs the same
  tool, but a list of each host's specific vulnerabilities is a list of targets.
  Decide with InCommon security whether the details page names vulnerabilities or
  only the grade and its general reasons, such as "TLS 1.0 enabled".
- **Tests.**
  - Parsing testssl.sh JSON fixtures: A+, a capped B, F, the trust and name-mismatch
    marks, a host that refuses, and a post-quantum host.
  - Worst-host aggregation, and the rescan schedule.
  - The deadline and per-run cap, and opted-out hosts skipped.
  - A private address refused before testssl.sh runs.
  - The badges on chips, including the floating key's rows.

## Proxies: tell which SPs are proxies, and of what kind

Many registered SPs are not the service people use but a proxy in front of it: a SAML
bridge to other protocols or other services (SATOSA, CILogon, Globus Auth, eduTEAMS,
Keycloak or SimpleSAMLphp as a hub), or a library access proxy (EZproxy, OpenAthens).
Users reach the real service through it, and what it asks for and releases is the
proxy's policy, not the service's. Flag them, with the kind where it can be told:

- **Signals in metadata.** An entity with both an `SPSSODescriptor` and an
  `IdPSSODescriptor` is almost always a proxy, but ingest drops the IdP role: keep
  whether an entity has both. Endpoint paths and host names name some proxies
  (`/ezproxy`, `login.ezproxy.`, `sp.openathens.net`, SATOSA's `/<backend>/acs/post`,
  Keycloak's `/realms/<realm>/broker/`). The display name, description and
  organization often say "proxy", "gateway" or "bridge". Check for any entity
  attribute or category that marks a proxy (AARC's guidance, REFEDS) before
  inventing one.
- **Kinds worth telling apart:** library access proxy, protocol bridge (SAML to OIDC
  or OAuth), research collaboration hub (one SP fronting many services), and a
  commercial identity broker (Okta, Entra ID, Auth0 used as the SP).
- **Signal it with its evidence.** A guess from a host name is weaker than both
  roles in one entity, so show "proxy (likely)" with what it was read from, as a
  badge, a facet and an API field. Let curation (`curation/curation.json`) confirm
  or overrule it.
- **Effect on other checks.** A proxy's login host being alive says nothing about the
  services behind it, and its requested attributes are the hub's, so say so where
  those are shown.

## SAML software: tell what each SP runs, where it can be told

Which software an SP runs decides which login routes work (see the access button's
"Which routes really log people in" above), what to expect of its metadata, and
which SPs a vulnerability advisory touches. Read it from what the SP already
publishes, and say how sure the guess is:

- **Endpoint paths** are the strongest public signal, but ingest keeps only the ACS
  bindings and hosts, so keep the ACS, request initiator and discovery response URLs.
  Known shapes: `/Shibboleth.sso/` (Shibboleth SP, and EZproxy and others that embed
  it), `/simplesaml/module.php/saml/sp/` (SimpleSAMLphp), `/mellon/` (mod_auth_mellon),
  `/realms/<realm>/broker/` (Keycloak), `/adfs/ls/` (AD FS), `/Saml2/Acs` (Sustainsys),
  plus vendor patterns (Canvas, Blackboard, Salesforce, Okta, Entra ID) to collect
  from the live aggregate.
- **entityID shapes:** `https://<host>/shibboleth` is the Shibboleth SP default, and
  `.../module.php/saml/sp/metadata.php/<name>` is SimpleSAMLphp's.
- **Metadata habits:** Shibboleth's `init:RequestInitiator` and
  `idpdisc:DiscoveryResponse` extensions, the NameID formats offered, and how keys are
  published.
- **From the health checks,** which already contact each login host: the `Server`
  header and the status page each package serves (`/Shibboleth.sso/Status` is usually
  local-only). Read only what one ordinary request returns; never probe for versions.
- **Show it** as "Shibboleth SP (from its endpoints)" on the service page, as a facet
  and in the API, and report a count per package. Never show a version: it would
  list who is behind on patches. That belongs in a private report, if anywhere.
