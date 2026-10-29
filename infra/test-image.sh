#!/bin/sh
# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

# Smoke test for the container image (`make test-image`; needs Docker):
#   1. the image holds both programs, xmlsec1, and the pinned signing certificates
#   2. each certificate verifies its federation's aggregate and rejects the other's
#      (uses the real aggregates in data/cache; skipped if they are not there)
#   3. ingest builds a snapshot from the test fixtures
#   4. the API serves the site and the snapshot from the image
set -eu
IMAGE=${IMAGE:-incatalog:local}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d)
NAME="incatalog-smoke-$$"
SERVER_PID=""
cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT INT TERM
fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { echo "ok   $*"; }
run() { docker run --rm --platform linux/arm64 "$@"; }

# 1. Contents
run --entrypoint /usr/local/bin/catalog-ingest "$IMAGE" --help >/dev/null || fail "catalog-ingest does not run"
run --entrypoint xmlsec1 "$IMAGE" --version >/dev/null || fail "xmlsec1 is missing"
ok "both programs and xmlsec1 run"
run --entrypoint sh "$IMAGE" -c 'grep -q "<h2 id=\"MIT\"" /usr/share/doc/incatalog/THIRD-PARTY-LICENSES.html && test -s /usr/share/doc/incatalog/NOTICE && test -s /usr/share/doc/incatalog/LICENSE' ||
  fail "the license notices are missing from the image"
ok "LICENSE, NOTICE and the third-party notices are in the image"
fingerprint() {
  run --entrypoint sh "$IMAGE" -c "openssl x509 -in /etc/incatalog/certs/$1 -noout -fingerprint -sha256 2>/dev/null || cat /etc/incatalog/certs/$1" |
    sed -n 's/.*Fingerprint=//p'
}
# Pinned in certs/README.md. If openssl is not in the image, compare on the host instead.
expect_fp() {
  got=$(fingerprint "$1")
  [ -n "$got" ] || got=$(openssl x509 -in "$ROOT/certs/$1" -noout -fingerprint -sha256 | sed -n 's/.*Fingerprint=//p')
  [ "$got" = "$2" ] || fail "$1 fingerprint is $got, expected $2"
}
expect_fp incommon-mdq.pem 60:49:74:D6:1F:E0:D7:F4:D6:3D:6C:8D:B9:8A:85:7E:64:2A:B9:B4:70:E3:E8:5D:D5:4D:66:3D:04:96:F9:00
expect_fp edugain-mds-v2.pem BD:21:40:48:9A:9B:D7:40:44:DD:68:05:34:F7:78:88:A9:C1:3B:0A:C1:7C:4F:3A:03:6E:0F:EC:6D:89:99:95
ok "signing certificates match their pinned fingerprints"

# 2. Signatures, both ways
verify() {
  run -v "$ROOT/data/cache:/c:ro" --entrypoint xmlsec1 "$IMAGE" --verify \
    --pubkey-cert-pem "/etc/incatalog/certs/$2" \
    --id-attr:ID urn:oasis:names:tc:SAML:2.0:metadata:EntitiesDescriptor "/c/$1" >/dev/null 2>&1
}
if [ -f "$ROOT/data/cache/incommon.xml" ] && [ -f "$ROOT/data/cache/edugain.xml" ]; then
  verify incommon.xml incommon-mdq.pem || fail "InCommon aggregate does not verify"
  verify edugain.xml edugain-mds-v2.pem || fail "eduGAIN aggregate does not verify"
  ! verify incommon.xml edugain-mds-v2.pem || fail "InCommon aggregate verified with the wrong certificate"
  ! verify edugain.xml incommon-mdq.pem || fail "eduGAIN aggregate verified with the wrong certificate"
  ok "aggregates verify with their own certificate and not the other's"
else
  echo "skip signature check: run 'make ingest' first to cache the real aggregates"
fi

# 3. Ingest from the fixtures (unsigned, so verification is off for this step)
FIX="$ROOT/crates/catalog-core/tests/fixtures"
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
python3 -m http.server "$PORT" --bind 0.0.0.0 --directory "$FIX" >/dev/null 2>&1 &
SERVER_PID=$!
sleep 1
HOST="http://host.docker.internal:$PORT"
run --add-host=host.docker.internal:host-gateway -v "$WORK:/out" --user "$(id -u)" \
  -e CATALOG_ENRICH_MODE=off -e CATALOG_SKIP_LOGOS=true \
  --entrypoint sh "$IMAGE" -c "unset CATALOG_INCOMMON_CERT CATALOG_EDUGAIN_CERT; exec catalog-ingest \
    --cache-dir /tmp/cache --out /out/catalog.json.gz \
    --incommon-url $HOST/incommon.xml --edugain-url $HOST/edugain.xml --federations-url $HOST/feds.json" \
  >"$WORK/ingest.log" 2>&1 || { cat "$WORK/ingest.log" >&2; fail "ingest from fixtures failed"; }
services=$(python3 -c 'import gzip,json,sys; print(len(json.load(gzip.open(sys.argv[1]))["services"]))' "$WORK/catalog.json.gz")
[ "$services" -gt 0 ] || fail "the snapshot has no services"
ok "ingest built a snapshot from the fixtures ($services services)"

# 4. Serve it
# The image runs as uid 1000, and mktemp -d made $WORK 0700 for the user running this
# script (uid 1001 on a GitHub runner). Docker Desktop ignores that on macOS bind
# mounts; Linux does not, and the API could not read the snapshot.
chmod -R a+rX "$WORK"
docker run -d --platform linux/arm64 --name "$NAME" -p 127.0.0.1::8080 -v "$WORK:/data:ro" \
  -e CATALOG_SOURCE=/data/catalog.json.gz "$IMAGE" >/dev/null
URL="http://$(docker port "$NAME" 8080 | head -1)"
i=0
until curl -fs -o /dev/null "$URL/v1/meta"; do
  i=$((i + 1)); [ $i -lt 30 ] || { docker logs "$NAME" >&2; fail "the API did not start"; }; sleep 0.5
done
curl -fs "$URL/" | grep -q "<title>" || fail "the site is not served"
total=$(curl -fs "$URL/v1/services?per_page=1" | python3 -c 'import json,sys; print(json.load(sys.stdin)["total"])')
[ "$total" = "$services" ] || fail "the API lists $total services, the snapshot has $services"
ok "the API serves the site and all $total services"
echo "image smoke test passed"
