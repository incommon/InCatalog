# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

# One image for both programs. In AWS it runs as two Lambda functions (the function's
# image_config picks the entry point); anywhere else it is an ordinary web server:
#
#   docker run -p 8080:8080 -v "$PWD/data:/data" -e CATALOG_SOURCE=/data/catalog.json.gz incatalog
#
# Built on Amazon Linux 2023, the OS Lambda runs, so the binaries and xmlsec1 match it.

FROM public.ecr.aws/amazonlinux/amazonlinux:2023 AS build
RUN dnf install -y --setopt=install_weak_deps=False gcc gcc-c++ make cmake tar gzip \
    && dnf clean all
# The toolchain pinned in rust-toolchain.toml.
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.96.1
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p catalog-api -p catalog-ingest \
    && mkdir /out && cp target/release/catalog-api target/release/catalog-ingest /out/

FROM public.ecr.aws/amazonlinux/amazonlinux:2023-minimal
# xmlsec1 checks the federations' signatures on each aggregate during ingest.
RUN microdnf install -y --setopt=install_weak_deps=0 xmlsec1 xmlsec1-openssl ca-certificates \
    && microdnf clean all
COPY --from=build /out/ /usr/local/bin/
COPY web /srv/web
# Signing certificates for the two aggregates. See certs/README.md for where they came
# from and how their fingerprints were checked.
COPY certs/*.pem /etc/incatalog/certs/
ENV CATALOG_STATIC_DIR=/srv/web \
    CATALOG_INCOMMON_CERT=/etc/incatalog/certs/incommon-mdq.pem \
    CATALOG_EDUGAIN_CERT=/etc/incatalog/certs/edugain-mds-v2.pem \
    PORT=8080 \
    BIND_ADDR=0.0.0.0 \
    RUST_LOG=info
USER 1000
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/catalog-api"]
