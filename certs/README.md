# Metadata signing certificates

Ingest checks each aggregate's XML signature against these with `xmlsec1` and stops
if either fails. Both verified the live aggregates on 2026-10-28, and each rejected
the other federation's aggregate.

| File | Signs | Source | SHA-256 fingerprint |
|---|---|---|---|
| `incommon-mdq.pem` | `https://mdq.incommon.org/entities` | InCommon's `inc-meta` repository, `mdx/us_incommon/inc-md-cert-mdq.pem` (`CN=mdq.incommon.org`, expires 2038-11-10) | `60:49:74:D6:1F:E0:D7:F4:D6:3D:6C:8D:B9:8A:85:7E:64:2A:B9:B4:70:E3:E8:5D:D5:4D:66:3D:04:96:F9:00` |
| `edugain-mds-v2.pem` | `https://mds.edugain.org/edugain-v2.xml` | `https://technical.edugain.org/mds-v2.cer` (`CN=eduGAIN RSA Signer CA 2022`, expires 2042-03-08) | `BD:21:40:48:9A:9B:D7:40:44:DD:68:05:34:F7:78:88:A9:C1:3B:0A:C1:7C:4F:3A:03:6E:0F:EC:6D:89:99:95` |

The eduGAIN fingerprint matches the one published at
`https://technical.edugain.org/metadata`. The InCommon fingerprint has not yet been
checked against an independently published value: compare it with InCommon's
current metadata signing certificate page before relying on it.

Check a file with `openssl x509 -in <file> -noout -fingerprint -sha256`.
