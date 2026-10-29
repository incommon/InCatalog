# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

output "site_url" {
  value = "https://${local.custom_domain ? var.site_domain : aws_cloudfront_distribution.site.domain_name}"
}

output "api_url" {
  value = aws_apigatewayv2_api.api.api_endpoint
}

output "ecr_repository_url" {
  value = aws_ecr_repository.app.repository_url
}

output "image_uri" {
  value = local.image_uri
}

output "data_bucket" {
  value = aws_s3_bucket.data.id
}

output "first_ingest_command" {
  description = "The API needs one snapshot before it can start; run this once after the first apply."
  value       = "aws lambda invoke --region ${var.region} --function-name ${aws_lambda_function.ingest.function_name} /dev/stdout"
}
