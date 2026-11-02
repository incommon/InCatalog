#!/bin/sh
# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

# Create the S3 bucket that holds OpenTofu state for this account (once), and
# write infra/opentofu/backend.hcl to point at it. Versioned, encrypted, private.
# Safe to rerun: it sets the lifecycle rule on an existing bucket too.
set -eu
account=$(aws sts get-caller-identity --query Account --output text)
region=${AWS_REGION:-us-east-2}
bucket="catabasis-tfstate-$account"
if aws s3api head-bucket --bucket "$bucket" 2>/dev/null; then
  echo "s3://$bucket already exists"
else
  aws s3api create-bucket --bucket "$bucket" --region "$region" \
    --create-bucket-configuration LocationConstraint="$region" >/dev/null
  aws s3api put-public-access-block --bucket "$bucket" --public-access-block-configuration \
    BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
  aws s3api put-bucket-versioning --bucket "$bucket" --versioning-configuration Status=Enabled
  aws s3api put-bucket-encryption --bucket "$bucket" --server-side-encryption-configuration \
    '{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"}}]}'
  aws s3api put-bucket-tagging --bucket "$bucket" --tagging 'TagSet=[{Key=Application,Value=catabasis}]'
  echo "created s3://$bucket"
fi
# A state version is the rollback for a bad apply, so keep the 20 newest of each
# object whatever their age, and older ones a week, as the data bucket does.
aws s3api put-bucket-lifecycle-configuration --bucket "$bucket" --lifecycle-configuration \
  '{"Rules":[{"ID":"age-out-old-versions","Status":"Enabled","Filter":{"Prefix":""},
    "NoncurrentVersionExpiration":{"NoncurrentDays":7,"NewerNoncurrentVersions":20},
    "Expiration":{"ExpiredObjectDeleteMarker":true},
    "AbortIncompleteMultipartUpload":{"DaysAfterInitiation":7}}]}'
echo "set the lifecycle rule on s3://$bucket"
cat > "$(dirname "$0")/opentofu/backend.hcl" <<HCL
bucket       = "$bucket"
key          = "catabasis/terraform.tfstate"
region       = "$region"
encrypt      = true
use_lockfile = true
HCL
echo "wrote infra/opentofu/backend.hcl"
