# Contributing — InCatalog

## Branching model: gitflow

We use [gitflow](https://nvie.com/posts/a-successful-git-branching-model/):

| Branch | Purpose |
|---|---|
| `main` | Production-ready releases only. Every commit on `main` is a tagged release. |
| `develop` | Integration branch and the default branch; the target for feature work. |
| `feature/<short-name>` | New work, branched from and merged back to `develop`. |
| `release/<version>` | Release stabilization, branched from `develop`, merged to `main` **and** `develop`. |
| `hotfix/<short-name>` | Urgent production fixes, branched from `main`, merged to `main` **and** `develop`. |

Rules:

- No direct commits to `main` or `develop` — everything arrives by merge from a
  feature/release/hotfix branch. `main` is protected: the CI checks below must pass on
  an up-to-date branch, for admins too, and it cannot be force-pushed or deleted.
- Merges to `main` are tagged `v<semver>`.
- Keep feature branches short-lived and focused; rebase on `develop` before merging.
- Commit messages: imperative summary line ≤ 72 chars; body explains *why* when it isn't
  obvious.

## Versioning

One component, on [SemVer 2.0.0](https://semver.org): the workspace version in
`Cargo.toml`, tagged `v<semver>` on `main`. The public contract is the HTTP API (its
OpenAPI document) and the snapshot format (`schema_version`); a backward-incompatible
change to either is MAJOR. Additive work is MINOR; fixes are PATCH. While the version is
`0.x`, minors may break (SemVer §4).

That one version is read everywhere, never written twice: the API reports it in
`/v1/meta` (`version`) and the OpenAPI `info.version`, the site's footer shows it, and
the image carries it as its `org.opencontainers.image.version` label (`make image` and
`build-images.yml` pass it to the Dockerfile, which refuses any other).

**Release notes** live in [`CHANGELOG.md`](CHANGELOG.md), in
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) style. A PR with a change
users or API clients would notice adds a line under `## [Unreleased]` (Added, Changed,
Deprecated, Removed, Fixed, Security). The API embeds the file when it is built and
serves it per release at `/v1/releases` and `/v1/releases/{version}`, and the footer's
version links to `#/releases/<version>`. So the notes always match the running build,
and a test fails if the `Cargo.toml` version has no section.

**Cutting a release** under gitflow, for version `X.Y.Z`:

1. Branch `release/X.Y.Z` from `develop`.
2. Set `version = "X.Y.Z"` in the root `Cargo.toml` (every crate inherits it) and run
   `cargo check` so `Cargo.lock` follows.
3. In `CHANGELOG.md`, rename `## [Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD` (the
   release date), add an empty `## [Unreleased]` above it, and update the link
   definitions at the bottom.
4. Open a PR from `release/X.Y.Z` into `main`. CI's `release notes / version` check
   fails unless `CHANGELOG.md` has a dated section for the `Cargo.toml` version and
   `vX.Y.Z` is not tagged yet. Only fixes for the release land on the branch now.
5. Merge it, then tag the merge commit on `main` and push the tag:
   `git tag -s vX.Y.Z -m "InCatalog X.Y.Z" && git push origin vX.Y.Z`. The tag builds
   the image and pushes it as `:vX.Y.Z` as well as `:<sha>`; deploying it is still the
   manual promotion `build-images.yml` describes.
6. Merge `release/X.Y.Z` back into `develop` (by PR), so `develop` has the bump and the
   dated section.

A hotfix is the same from `hotfix/<name>` off `main`, with a PATCH bump.

## Testing

- **Unit tests** for the logic in every crate, and **the pipeline test**
  (`crates/catalog-core/tests/pipeline.rs`) over the fixture aggregates.
- **The image smoke test** (`make test-image`, `infra/test-image.sh`): contents, pinned
  certificate fingerprints, signature checks when the real aggregates are cached, an
  ingest from the fixtures, and serving.
- **Coverage gates:** CI enforces three thresholds on the Rust workspace with
  `cargo llvm-cov`, read from `.coverage-threshold` (`lines`, `regions`, `functions`).
  That file is authoritative and carries the measured figures and the date; this
  document deliberately doesn't repeat the numbers, so they can't drift apart.

  **Ratchet up, never down.** Keep a little slack under each measured figure, sized in
  misses rather than points, and re-measure and raise when a change moves a number
  meaningfully:

  ```bash
  cargo llvm-cov --workspace --ignore-filename-regex '(^|/)main\.rs$' --summary-only
  ```

  **Branch coverage is not measured**, and therefore not gated: it needs nightly.
  `regions` is the stable-toolchain stand-in.

  `main.rs` entrypoints are excluded from all three. Corollary: a `main.rs` holds only
  process bootstrap (args, env, bind, serve) — logic lives in the crate's library, where
  it is tested.

### What CI runs, and when

CI (`.github/workflows/ci.yml`) runs on every PR into `develop`, `release/*`, and `main`,
and on every push to `develop` and `main`. **A red build blocks merge — no exceptions.**

| Check | What it does | Runs |
|---|---|---|
| `fmt / clippy / test / coverage` | `cargo fmt --check`, `clippy -D warnings`, `cargo test`, coverage gates | Rust-relevant changes; reports on every PR |
| `terraform fmt / validate` | `fmt -check`, `validate` with `-lockfile=readonly`, at `.terraform-version` | Always |
| `license policy / third-party notices` | `cargo deny check licenses`; the third-party notices generate | Always |
| `release notes / version` | On a PR into `main`: `CHANGELOG.md` has a dated section for the `Cargo.toml` version, and that version is not tagged yet (§ Versioning) | Always; checks only PRs into `main` |

The first two are **required** by `main`'s branch protection, by those exact names.
`release notes / version` always reports, so it can be made required too.

**Rust-relevant** is wider than `*.rs`: anything under `crates/` (fixtures included),
`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.coverage-threshold`, and
`.github/workflows/ci.yml` (a CI change must prove itself), and `CHANGELOG.md`, which
the API embeds and a test parses. Other Markdown never counts. Pushes
to `develop`/`main` always run everything.

Two rules for anyone editing `ci.yml`:

- **The required check is `rust-gate`, named `fmt / clippy / test / coverage`.** It
  always runs and reports on behalf of the real `Rust suite` job. Renaming it breaks
  `main`'s branch protection.
- **Never convert it to a workflow-level `paths:` filter.** A required check that never
  reports leaves the PR permanently unmergeable.

`make lint` and `make test` run the same checks locally.

## Dependencies and security

- **License policy** (`deny.toml`): permissive licenses only. MPL-2.0 is allowed for
  four named crates of Servo's CSS engine (via `scraper`), not tree-wide; a new
  dependency outside the list fails CI until it is reviewed.
- **Third-party notices:** the image build writes `THIRD-PARTY-LICENSES.html` from the
  `Cargo.lock` it builds and ships it in `/usr/share/doc/incatalog/`, so it is never
  committed and a dependency bump needs no follow-up. CI checks that it generates (every
  license accepted in `about.toml`, every clarification still matching) and attaches it
  to the run. `make notices` writes it locally.
- **Security audit** (`.github/workflows/security-audit.yml`): a weekly RustSec scan
  (`cargo deny check advisories sources`), not a PR gate. An advisory with no upstream
  fix is ignored in `deny.toml` by ID, with a rationale and a re-review date.
- **Dependabot** (`.github/dependabot.yml`) opens weekly PRs against `develop` for
  crates (minor/patch grouped) and GitHub Actions. Dependabot alerts and security
  updates are on for the repository.

## Container image and deploys

One `linux/arm64` image runs both Lambda functions. `.github/workflows/build-images.yml`
builds it on a native arm64 runner, runs the smoke test, and pushes it to ECR:

- **Push to `develop`** → pushes `:<sha>`. Nothing deploys.
- **Run workflow** (manual) → also moves `:latest`. Terraform deploys the digest
  `:latest` points at, so the next `terraform apply` ships it.

It authenticates **keyless via GitHub OIDC**, assuming the role Terraform creates when
`github_oidc_provider_arn` is set, scoped to this repository's `dev` GitHub Environment.
That environment holds two variables: `AWS_ROLE_ARN` (the `github_image_push_role_arn`
output) and `ECR_REPOSITORY`. Until both are set the workflow builds and tests but does
not push. No long-lived AWS keys are stored in GitHub.

`make deploy` remains the way to build, push and apply from a workstation.
