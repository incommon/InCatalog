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

The first two are **required** by `main`'s branch protection, by those exact names.

**Rust-relevant** is wider than `*.rs`: anything under `crates/` (fixtures included),
`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.coverage-threshold`, and
`.github/workflows/ci.yml` (a CI change must prove itself). Markdown never counts. Pushes
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
