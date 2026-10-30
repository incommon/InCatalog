#!/bin/sh
# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

# Create the S3 bucket that holds OpenTofu state for this account (once), and
# write infra/terraform/backend.hcl to point at it. Versioned, encrypted, private.
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
cat > "$(dirname "$0")/terraform/backend.hcl" <<HCL
bucket       = "$bucket"
key          = "catabasis/terraform.tfstate"
region       = "$region"
encrypt      = true
use_lockfile = true
HCL
echo "wrote infra/terraform/backend.hcl"
