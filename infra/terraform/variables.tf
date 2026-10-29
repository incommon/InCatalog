# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

variable "name" {
  description = "Prefix for every resource."
  type        = string
  default     = "catabasis"
}

variable "region" {
  type    = string
  default = "us-east-2"
}

variable "tags" {
  type    = map(string)
  default = {}
}

variable "image_tag" {
  description = "Tag of the image in ECR to deploy (pushed by `make push`). Deploys pin its digest."
  type        = string
  default     = "latest"
}

variable "ingest_schedule" {
  description = "When to rebuild the snapshot (UTC). InCommon and eduGAIN republish at least daily."
  type        = string
  default     = "cron(20 7 * * ? *)"
}

variable "curation_file" {
  description = "Optional local path to a curation overlay JSON, uploaded beside the snapshot."
  type        = string
  default     = ""
}

variable "site_domain" {
  description = "Hostname for the site, such as catalog.example.org. Empty serves it at the CloudFront default *.cloudfront.net name."
  type        = string
  default     = ""
}

variable "site_zone" {
  description = "Route 53 public hosted zone in this account that holds site_domain. Terraform validates the certificate and adds the alias records there."
  type        = string
  default     = ""
}

variable "extra_cors_origins" {
  description = "Origins besides the CloudFront site allowed to call the API (e.g. http://localhost:8080)."
  type        = list(string)
  default     = []
}

variable "log_retention_days" {
  type    = number
  default = 30
}

variable "claude_model" {
  description = "Bedrock inference profile for reading searches, choosing logos, and writing summaries. us. keeps requests on US infrastructure; global. is 10% cheaper and may route anywhere. The account needs Bedrock access to the model."
  type        = string
  default     = "us.anthropic.claude-haiku-5-5"
}

variable "logo_max_lookups" {
  description = "Most organization websites to look up per ingest run. Results are cached for 30 days."
  type        = number
  default     = 150
}

variable "summary_max_per_run" {
  description = "Most summaries to write per ingest run. Bedrock has no batch API, so they are written during the run; the daily trickle of metadata changes is far below this."
  type        = number
  default     = 500
}

variable "github_oidc_provider_arn" {
  description = "ARN of this account's IAM OIDC provider for token.actions.githubusercontent.com. Set it to create the role .github/workflows/build-images.yml assumes to push images; empty creates nothing."
  type        = string
  default     = ""
}

variable "github_repository" {
  description = "The GitHub repository (owner/name) whose image builds may assume the push role."
  type        = string
  default     = "incommon/InCatalog"
}

variable "github_environment" {
  description = "The GitHub Environment the image build runs in. The push role trusts only that environment's jobs."
  type        = string
  default     = "dev"
}
