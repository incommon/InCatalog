# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

.PHONY: test lint notices ingest dev image test-image run-image state-bucket init push plan deploy seed first-ingest

SNAPSHOT ?= data/catalog.json.gz

test:
	cargo test --workspace

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	terraform -chdir=infra/terraform fmt -check
	cargo deny check

# Regenerate the dependencies' license notices after Cargo.lock changes (CI checks them).
notices:
	cargo fetch --locked
	cargo about generate --offline --locked about.hbs -o THIRD-PARTY-LICENSES.html

# Fetch InCommon + eduGAIN metadata and write a local snapshot (~10 s on a warm cache).
ingest:
	cargo run --release -p catalog-ingest -- --cache-dir data/cache --out $(SNAPSHOT) \
		$(if $(wildcard curation/curation.json),--curation curation/curation.json)

# API and web front end together on http://localhost:8080
dev: $(SNAPSHOT)
	CATALOG_SOURCE=$(SNAPSHOT) CATALOG_STATIC_DIR=web cargo run --release -p catalog-api

$(SNAPSHOT):
	$(MAKE) ingest

# ---------- AWS ----------
# One container image runs both Lambda functions. Terraform state lives in S3
# (backend.hcl); `make state-bucket` creates the bucket once per account.
AWS_PROFILE ?= incommon-hyperdrive-dev
AWS_REGION ?= us-east-2
export AWS_PROFILE AWS_REGION
TF := terraform -chdir=infra/terraform
IMAGE := incatalog:local
TAG := $(shell date -u +%Y%m%d-%H%M%S)

image:
	docker buildx build --platform linux/arm64 --load -t $(IMAGE) .

# Smoke test for the image: contents, signatures, ingest from fixtures, serving.
test-image: image
	IMAGE=$(IMAGE) ./infra/test-image.sh

# The image runs the site and API locally too, against your snapshot.
run-image: image
	docker run --rm -p 8080:8080 -v "$$PWD/data:/data:ro" -e CATALOG_SOURCE=/data/catalog.json.gz $(IMAGE)

state-bucket:
	./infra/state-bucket.sh

init:
	$(TF) init -input=false -reconfigure -backend-config=backend.hcl

# The repository has to exist before the first push, and the functions need an image.
push: image init
	$(TF) apply -input=false -target=aws_ecr_repository.app -target=aws_ecr_lifecycle_policy.app
	repo=$$($(TF) output -raw ecr_repository_url) && \
	aws ecr get-login-password | docker login --username AWS --password-stdin $${repo%%/*} && \
	docker tag $(IMAGE) $$repo:$(TAG) && docker tag $(IMAGE) $$repo:latest && \
	docker push -q $$repo:$(TAG) && docker push -q $$repo:latest

plan: init
	$(TF) plan

deploy: push
	$(TF) apply

# Copy the local summary and logo caches up before the first ingest, so AWS reuses
# the summaries already written instead of generating them all again. Skips any
# that already exist in the bucket.
seed:
	bucket=$$($(TF) output -raw data_bucket) && \
	for f in summary-cache.json logo-cache.json; do \
	  if aws s3api head-object --bucket $$bucket --key snapshots/$$f >/dev/null 2>&1; then echo "$$f already in s3://$$bucket"; \
	  else aws s3 cp data/$$f s3://$$bucket/snapshots/$$f; fi; done

first-ingest:
	$$($(TF) output -raw first_ingest_command)
