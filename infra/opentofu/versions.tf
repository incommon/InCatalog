# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

terraform {
  required_version = ">= 1.10"
  # S3 state with native locking. Settings come from backend.hcl (`make init`).
  backend "s3" {}
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = var.region
  default_tags {
    tags = merge({ Application = var.name }, var.tags)
  }
}

# CloudFront certificates must live in us-east-1.
provider "aws" {
  alias  = "us_east_1"
  region = "us-east-1"
  default_tags {
    tags = merge({ Application = var.name }, var.tags)
  }
}
