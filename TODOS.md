# TODOs

## A "Log in" button on each service's expanded view

Let someone go from a service's page straight to signing in to it, through their own
institution. Metadata gives three routes. Each works for only some SPs, so the button
has to say which route it uses and show only when one exists.

The button shows only on a service that is all of these. Anything else gets no button,
not even the fallback:

- **Multilateral.** Its sharing label (#16) is `multilateral`. A `bilateral` or
  `unknown` one gets none.
- **Alive.** Its health `status` is `alive`. A `dead` or `unknown` one gets none.
- **Not filtered by InCommon.** It has no `import_filter`: InCommon's eduGAIN import
  didn't remove it. An InCommon IdP can't see a filtered SP, so a login would fail.
- **Not OFAC-listed.** It has no OFAC entity match, possible or confirmed (see the OFAC
  TODO). A match that curation dismissed doesn't count. Being in a country under an
  OFAC program, with no entity match, doesn't hide it.

Check all four in one place in `catalog-core`, and serve the result with the service,
so the site and the API agree on when it shows.

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
- **Who it can work for.** Even a multilateral SP still depends on the user's IdP
  releasing the attributes it needs. Say that on the button, as the About page does,
  rather than promising access.
- **No open redirect.** Build every URL only from metadata and the chosen IdP's
  metadata, never from query input. Allow only `https:`, and refuse private and
  loopback hosts, as the logo fetcher does.
- **Tests.** URL construction for each route, which button shows for each kind of SP,
  that no button shows when any one of the four conditions fails, and that a crafted
  entityID or parameter can't send the browser anywhere else.

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
  distinct from the other markings, so no red (InCommon-filtered services, `--filtered`)
  and no motion (the dead services' moving zebra ring). Make its colors theme tokens that work
  in dark mode.
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

The service chips and cards are gaining markings: the red outline and badge for
services InCommon filtered from its import (`.card.filtered`, already shipped), the
the dead services' zebra-striped white and black ring (`.card.dead`, already shipped), and the OFAC style.
Each TODO says to explain its marking in a legend. Make that one legend: a key floating in the lower-right corner of the chip
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

  Add keyboard-only Playwright tests that tab through each page. They check that every
  control can be reached and operated, and that the floating key never covers the
  focused element (2.4.11, Focus Not Obscured).
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

## Center logos and fill their tiles, with a small even margin

Every logo should sit centered in its tile and fill it as fully as its shape allows,
with only a small, even margin around the artwork. Two examples to start from:
- **Penn State.** Its metadata logos are broken, so its cards show `psu-mark.svg`,
  which the logo fetcher found on psu.edu.
- **Data Intensive Research Initiative of South Africa (DIRISA)**
  (`https://dirisa.ac.za/dirisa-sp`, registered by SAFIRE). Its only logo is a
  649×240 metadata PNG served from SAFIRE's MDUI cache, a wordmark about 2.7 times as
  wide as it is tall. It works, so the website lookup never runs for it.

The tile already centers the image (`.logo img` in `web/styles.css`: `object-fit:
contain` inside a 4px inset). So a logo that looks small or off-center has the problem
inside the file:
- **Built-in margins.** Transparent or white space baked into the image makes the
  mark small, and lopsided when the space is uneven.
- **SVG geometry.** A `viewBox` larger than the artwork does the same. So do `width`
  and `height` that don't match the `viewBox`'s shape, combined with a
  `preserveAspectRatio` such as `xMinYMid`, which pins the artwork to one side.
- **Wide wordmarks.** A 456×150 banner in a 40px square can only fill a third of it.

The fix:
- **Measure the artwork at ingest.** `check_image` in `catalog-ingest/src/logos.rs`
  already downloads each logo, up to 1 MiB, but `sniff` reads only its size. Decode
  rasters (PNG, JPEG, GIF, WebP, ICO) and find the bounding box of pixels that aren't
  background. Background is alpha below a threshold, or a near-uniform color along
  the border, usually white. For SVG, read the root's `viewBox`, `width`, `height`
  and `preserveAspectRatio`, and render it to find the artwork's box.
  - SVG comes from thousands of registrants, so render with no external references or
    fonts fetched, and cap the size and time.
  - New crates need `deny.toml`'s license review.
- **Store the box, not a new image.** Store the box on `Logo` and `DiscoveredLogo`, as
  fractions of the image's own size, and in `logo-cache.json`. Bump the cache's schema
  so existing entries are measured once. The site keeps loading the logo from its own
  URL, so nothing about rights, hosting or the CSP changes.
- **Crop and fit in the front end.** `logoEl` gets the box and positions the `<img>`
  inside the tile's `overflow: hidden`: scale the box to fit the tile minus the
  margin, center it, and offset the image so the box lands in the middle. Keep that
  arithmetic in one small pure function. `object-view-box` would do it in CSS, but
  not every browser supports it.
- **The margin.** Make it one value, a share of the tile (about 8% to 10%), so the
  40px card tile and the 64px service-page tile look alike. A logo with a full-bleed
  opaque background, such as a colored square, fills the tile edge to edge, clipped by
  its rounded corners, rather than floating in white.
- **Prefer a mark that fits the tile.**
  - **Among metadata logos.** `best_logo` in `catalog-core/src/search.rs` picks the
    largest area, which favors a wide banner over a square mark. Rank instead by how
    large the measured artwork would render in the tile, among logos big enough to
    look sharp.
  - **From the website lookup.** When it finds both a square mark and a wide
    wordmark, prefer the square one. Say so in the candidate list Claude picks from,
    and check the shape after the pick, as the size is checked now.
  - **When the only working logo is wide, as DIRISA's is.** Decide whether to also run
    the website lookup for a square mark. A working metadata logo always wins today,
    because it is the registrant's own choice. Whatever is decided, crop DIRISA's
    PNG to its artwork, so the wordmark is at least as large as its shape allows.
- **No box, no change.** A logo that couldn't be measured renders as today.
- **Tests.**
  - Fixtures: a PNG with transparent margins, one with white margins, artwork off to
    one side, an SVG with a padded `viewBox`, an SVG with `xMinYMid` and mismatched
    dimensions, a wide wordmark, a full-bleed opaque square, and an image that is all
    background.
  - Ingest: the stored box for each fixture, and an SVG with external references and
    an oversized SVG both refused safely.
  - Front end: the positioning function for each fixture, once the Playwright setup
    from the WCAG TODO exists.
  - `best_logo` choosing a square metadata mark over a larger wide banner.
  - Visual: a contact sheet of real logos (Penn State's and DIRISA's among them) in
    both tile sizes, to check by eye before and after.

## The InCommon logotype, with "powered by Internet2" under it

Show the InCommon logotype with the "powered by Internet2" sublogotype beneath it, as
one lockup. The site already uses InCommon's colors and typeface, but its only mark is
InCatalog's own: the inline SVG in `.brand` in `web/index.html`.

- **Official artwork only.** Get the lockup files from Internet2's brand or marketing
  team, as SVG, in every version the brand guidelines allow. Don't redraw, recolor or
  re-space them, and don't copy them from a website. Record where each file came from,
  its version and the date, beside the files (for example `web/brand/README.md`).
- **Follow the guidelines.** Clear space, minimum size, and which version goes on which
  background. "powered by Internet2" is the smaller line, so its smallest legible size
  sets the lockup's minimum height. If the lockup can't stay legible at phone width,
  use whichever smaller-screen arrangement the guidelines allow, rather than letting
  it shrink.
- **Where.** In the top bar, to the left of InCatalog's mark, separated by a thin
  rule, so it reads as an InCommon service. It links to incommon.org, and it is not
  part of the "InCatalog home" link. Decide whether the footer carries it too.
  - The lockup is two lines, so the top bar gets taller. Check the landing globe and
    the facets' sticky position still line up.
  - Below 520px the top bar stacks (`styles.css`). Check the lockup there and at
    320 CSS px.
- **Both themes.** Use the reversed version on dark backgrounds. The theme comes from
  `prefers-color-scheme` or from `data-theme` on the root, so a `<picture>` with a media
  query alone would miss `data-theme`. Swap the versions with the same selectors the
  theme tokens use. If the guidelines allow a single-color version, one inline SVG in
  `currentColor` is simpler.
- **Self-hosted.** Serve the files from the site's own origin (for example
  `web/brand/`), which the CSP's `img-src 'self'` already allows. Cache them like the
  other static assets.
- **Accessible.** One accessible name for the lockup: "InCommon, powered by Internet2".
  A logotype is exempt from WCAG's text-contrast minimums, but the link around it
  still needs a visible focus style and a big enough target. Add it to the pages the
  WCAG scan covers.
- **Trademarks aren't Apache-2.0.** The license excludes trademarks (§6), so say in
  `NOTICE` and the README's License section that the InCommon and Internet2 logotypes
  are Internet2's trademarks, used with permission, and not licensed with the code.
  Say too that a fork must remove them.
- **Check.** Look at it in both themes, at desktop width, at 520px and at 320 CSS px,
  against the guidelines' clear-space and minimum-size rules.

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
