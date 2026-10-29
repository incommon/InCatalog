# Copyright 2026 Internet2
# SPDX-License-Identifier: Apache-2.0

# InCatalog on AWS, all serverless:
#
#   EventBridge (daily) ─▶ Lambda catalog-ingest ─▶ S3 data/catalog.json.gz
#   CloudFront ─▶ S3 site (static web/)        browser ─▶ API Gateway ─▶ Lambda catalog-api ─▶ S3
#
# Both functions run the one container image in ECR (see the Dockerfile); each picks
# its program with an entry point. `make deploy` builds and pushes the image first.
#
# The API Lambda loads the snapshot on cold start and re-checks its ETag every few
# minutes, so a new ingest goes live without a deploy.

locals {
  snapshot_key = "snapshots/catalog.json.gz"
  curation_key = "curation/curation.json"
  # Logo checks and website lookups, kept between runs (catalog-ingest's default
  # location: beside the snapshot).
  logo_cache_key = "snapshots/logo-cache.json"
  # Generated summaries, keyed by a hash of each entry's metadata, plus any batch
  # still being processed by the Message Batches API.
  summary_cache_key = "snapshots/summary-cache.json"
  # Every Claude task (search reading, logo choice, summaries) goes through Bedrock
  # InvokeModel, signed with the function's role: there is no API key to store. The
  # model is a cross-region inference profile (us.…), which routes to the same
  # foundation model in several regions, so the role may invoke both.
  claude_env = {
    CATALOG_CLAUDE_PROVIDER = "bedrock-runtime"
  }
  claude_profile_arn      = "arn:aws:bedrock:${var.region}:${data.aws_caller_identity.current.account_id}:inference-profile/${var.claude_model}"
  claude_foundation_model = replace(var.claude_model, "/^(us|global|eu|apac|jp|au)\\./", "")
  web_root                = "${path.module}/../../web"
  mime = {
    html  = "text/html; charset=utf-8"
    css   = "text/css; charset=utf-8"
    js    = "text/javascript; charset=utf-8"
    json  = "application/json"
    svg   = "image/svg+xml"
    png   = "image/png"
    ico   = "image/x-icon"
    woff2 = "font/woff2"
    txt   = "text/plain; charset=utf-8"
  }
  web_files = [for f in fileset(local.web_root, "**") : f if f != "config.js" && !startswith(f, ".")]
}

# ---------- data bucket ----------

resource "aws_s3_bucket" "data" {
  bucket_prefix = "${var.name}-data-"
}

resource "aws_s3_bucket_public_access_block" "data" {
  bucket                  = aws_s3_bucket.data.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_versioning" "data" {
  bucket = aws_s3_bucket.data.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "data" {
  bucket = aws_s3_bucket.data.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_object" "curation" {
  count  = var.curation_file == "" ? 0 : 1
  bucket = aws_s3_bucket.data.id
  key    = local.curation_key
  source = var.curation_file
  etag   = filemd5(var.curation_file)
}

# ---------- container image ----------

resource "aws_ecr_repository" "app" {
  name                 = var.name
  image_tag_mutability = "MUTABLE"
  image_scanning_configuration {
    scan_on_push = true
  }
}

# Deploys pin the image by digest, so old tags are only needed for rollback.
resource "aws_ecr_lifecycle_policy" "app" {
  repository = aws_ecr_repository.app.name
  policy = jsonencode({
    rules = [{
      rulePriority = 1
      description  = "Keep the last 10 images"
      selection    = { tagStatus = "any", countType = "imageCountMoreThan", countNumber = 10 }
      action       = { type = "expire" }
    }]
  })
}

# The image `make push` just pushed. Pinning the digest makes each push a deploy.
data "aws_ecr_image" "app" {
  repository_name = aws_ecr_repository.app.name
  image_tag       = var.image_tag
}

locals {
  image_uri = "${aws_ecr_repository.app.repository_url}@${data.aws_ecr_image.app.image_digest}"
}

# ---------- CI image push (GitHub OIDC, no stored keys) ----------

# .github/workflows/build-images.yml assumes this role to push to the ECR
# repository above, and nothing else. Only jobs in one GitHub Environment of one
# repository can assume it. Created only when github_oidc_provider_arn is set:
# the provider is per account and usually exists already.
locals {
  github_push = var.github_oidc_provider_arn != "" ? 1 : 0
}

data "aws_iam_policy_document" "github_assume" {
  count = local.github_push
  statement {
    actions = ["sts:AssumeRoleWithWebIdentity"]
    principals {
      type        = "Federated"
      identifiers = [var.github_oidc_provider_arn]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:aud"
      values   = ["sts.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:sub"
      values   = ["repo:${var.github_repository}:environment:${var.github_environment}"]
    }
  }
}

resource "aws_iam_role" "github_image_push" {
  count              = local.github_push
  name               = "${var.name}-github-image-push"
  assume_role_policy = data.aws_iam_policy_document.github_assume[0].json
}

data "aws_iam_policy_document" "github_image_push" {
  count = local.github_push
  statement {
    actions   = ["ecr:GetAuthorizationToken"]
    resources = ["*"]
  }
  statement {
    actions = [
      "ecr:BatchCheckLayerAvailability",
      "ecr:BatchGetImage",
      "ecr:CompleteLayerUpload",
      "ecr:DescribeImages",
      "ecr:DescribeRepositories",
      "ecr:GetDownloadUrlForLayer",
      "ecr:InitiateLayerUpload",
      "ecr:PutImage",
      "ecr:UploadLayerPart",
    ]
    resources = [aws_ecr_repository.app.arn]
  }
}

resource "aws_iam_role_policy" "github_image_push" {
  count  = local.github_push
  name   = "ecr-push"
  role   = aws_iam_role.github_image_push[0].id
  policy = data.aws_iam_policy_document.github_image_push[0].json
}

# ---------- IAM ----------

data "aws_iam_policy_document" "lambda_assume" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "ingest" {
  name_prefix        = "${var.name}-ingest-"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume.json
}

resource "aws_iam_role" "api" {
  name_prefix        = "${var.name}-api-"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume.json
}

resource "aws_iam_role_policy_attachment" "ingest_logs" {
  role       = aws_iam_role.ingest.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

resource "aws_iam_role_policy_attachment" "api_logs" {
  role       = aws_iam_role.api.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

resource "aws_iam_role_policy" "ingest_s3" {
  role = aws_iam_role.ingest.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      { Effect = "Allow", Action = ["s3:PutObject"], Resource = "${aws_s3_bucket.data.arn}/${local.snapshot_key}" },
      { Effect = "Allow", Action = ["s3:GetObject"], Resource = "${aws_s3_bucket.data.arn}/${local.curation_key}" },
      { Effect = "Allow", Action = ["s3:GetObject", "s3:PutObject"], Resource = "${aws_s3_bucket.data.arn}/${local.logo_cache_key}" },
      { Effect = "Allow", Action = ["s3:GetObject", "s3:PutObject"], Resource = "${aws_s3_bucket.data.arn}/${local.summary_cache_key}" },
    ]
  })
}

data "aws_caller_identity" "current" {}

# The API reads searches with Claude; ingest chooses logos from organizations'
# websites and writes summaries. Both call Claude in Amazon Bedrock.
data "aws_iam_policy_document" "bedrock" {
  statement {
    actions = ["bedrock:InvokeModel"]
    resources = [
      local.claude_profile_arn,
      "arn:aws:bedrock:*::foundation-model/${local.claude_foundation_model}",
    ]
  }
}

resource "aws_iam_role_policy" "api_bedrock" {
  role   = aws_iam_role.api.id
  policy = data.aws_iam_policy_document.bedrock.json
}

resource "aws_iam_role_policy" "ingest_bedrock" {
  role   = aws_iam_role.ingest.id
  policy = data.aws_iam_policy_document.bedrock.json
}

# The API can only read the snapshot.
resource "aws_iam_role_policy" "api_s3" {
  role = aws_iam_role.api.id
  policy = jsonencode({
    Version   = "2012-10-17"
    Statement = [{ Effect = "Allow", Action = ["s3:GetObject"], Resource = "${aws_s3_bucket.data.arn}/${local.snapshot_key}" }]
  })
}

# ---------- ingest ----------

resource "aws_cloudwatch_log_group" "ingest" {
  name              = "/aws/lambda/${var.name}-ingest"
  retention_in_days = var.log_retention_days
}

resource "aws_lambda_function" "ingest" {
  function_name = "${var.name}-ingest"
  role          = aws_iam_role.ingest.arn
  package_type  = "Image"
  image_uri     = local.image_uri
  architectures = ["arm64"]
  image_config {
    entry_point = ["/usr/local/bin/catalog-ingest"]
  }
  # ~210 MiB of XML is streamed to /tmp and parsed one entity at a time. Checking
  # each aggregate's signature with xmlsec1 first peaks at ~700 MiB.
  memory_size = 2048
  # The first runs check ~1,900 logo URLs and look up websites; later runs mostly
  # hit the logo cache and finish in about a minute.
  timeout = 900
  ephemeral_storage {
    size = 1024
  }
  environment {
    variables = merge(
      {
        CATALOG_OUT       = "s3://${aws_s3_bucket.data.id}/${local.snapshot_key}"
        CATALOG_CACHE_DIR = "/tmp/catalog"
        RUST_LOG          = "info"
      },
      var.curation_file == "" ? {} : { CATALOG_CURATION = "s3://${aws_s3_bucket.data.id}/${local.curation_key}" },
      local.claude_env,
      {
        CATALOG_LOGO_MODEL       = var.claude_model
        CATALOG_LOGO_MAX_LOOKUPS = tostring(var.logo_max_lookups)
        CATALOG_ENRICH_MODEL     = var.claude_model
        # Bedrock has no Message Batches API, so summaries are written during the
        # run. This keeps a large backlog (a prompt change) inside the 15-minute limit;
        # the rest follows on later runs.
        CATALOG_ENRICH_MAX = tostring(var.summary_max_per_run)
      },
    )
  }
  depends_on = [aws_cloudwatch_log_group.ingest]
}

resource "aws_cloudwatch_event_rule" "ingest" {
  name                = "${var.name}-ingest"
  schedule_expression = var.ingest_schedule
}

resource "aws_cloudwatch_event_target" "ingest" {
  rule = aws_cloudwatch_event_rule.ingest.name
  arn  = aws_lambda_function.ingest.arn
}

resource "aws_lambda_permission" "ingest_schedule" {
  statement_id  = "AllowEventBridge"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.ingest.function_name
  principal     = "events.amazonaws.com"
  source_arn    = aws_cloudwatch_event_rule.ingest.arn
}

# ---------- API ----------

resource "aws_cloudwatch_log_group" "api" {
  name              = "/aws/lambda/${var.name}-api"
  retention_in_days = var.log_retention_days
}

resource "aws_lambda_function" "api" {
  function_name = "${var.name}-api"
  role          = aws_iam_role.api.arn
  package_type  = "Image"
  image_uri     = local.image_uri
  architectures = ["arm64"]
  # The image's default entry point is catalog-api.
  memory_size = 1024
  # Covers a Claude reading (12 s client timeout) plus a cold-start snapshot load.
  timeout = 20
  environment {
    variables = merge(
      {
        CATALOG_SOURCE       = "s3://${aws_s3_bucket.data.id}/${local.snapshot_key}"
        CATALOG_REFRESH_SECS = "300"
        CORS_ALLOW_ORIGIN    = join(",", concat([for d in local.site_domains : "https://${d}"], var.extra_cors_origins))
        RUST_LOG             = "info"
        CATALOG_NL_MODEL     = var.claude_model
      },
      local.claude_env,
    )
  }
  depends_on = [aws_cloudwatch_log_group.api]
}

resource "aws_apigatewayv2_api" "api" {
  name          = "${var.name}-api"
  protocol_type = "HTTP"
}

resource "aws_apigatewayv2_integration" "api" {
  api_id                 = aws_apigatewayv2_api.api.id
  integration_type       = "AWS_PROXY"
  integration_uri        = aws_lambda_function.api.invoke_arn
  payload_format_version = "2.0"
}

resource "aws_apigatewayv2_route" "api" {
  api_id    = aws_apigatewayv2_api.api.id
  route_key = "$default"
  target    = "integrations/${aws_apigatewayv2_integration.api.id}"
}

resource "aws_apigatewayv2_stage" "api" {
  api_id      = aws_apigatewayv2_api.api.id
  name        = "$default"
  auto_deploy = true
  default_route_settings {
    throttling_burst_limit = 100
    throttling_rate_limit  = 50
  }
}

resource "aws_lambda_permission" "api_gateway" {
  statement_id  = "AllowApiGateway"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.api.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_apigatewayv2_api.api.execution_arn}/*/*"
}

# ---------- static site ----------

resource "aws_s3_bucket" "site" {
  bucket_prefix = "${var.name}-site-"
}

resource "aws_s3_bucket_public_access_block" "site" {
  bucket                  = aws_s3_bucket.site.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_object" "web" {
  for_each     = toset(local.web_files)
  bucket       = aws_s3_bucket.site.id
  key          = each.value
  source       = "${local.web_root}/${each.value}"
  etag         = filemd5("${local.web_root}/${each.value}")
  content_type = lookup(local.mime, reverse(split(".", each.value))[0], "application/octet-stream")
  # Unversioned file names: make CloudFront and browsers revalidate (a cheap 304)
  # rather than serve last deploy's app.js for a day.
  cache_control = "no-cache"
}

resource "aws_s3_object" "config" {
  bucket        = aws_s3_bucket.site.id
  key           = "config.js"
  content       = "window.CATALOG_CONFIG = { apiBase: ${jsonencode(aws_apigatewayv2_api.api.api_endpoint)} };\n"
  content_type  = local.mime.js
  cache_control = "no-cache"
}

resource "aws_cloudfront_origin_access_control" "site" {
  name                              = "${var.name}-site"
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

# Metadata is written by thousands of registrants; a strict CSP means a hostile
# logo URL or description can at most fail to render.
resource "aws_cloudfront_response_headers_policy" "site" {
  name = "${var.name}-site"
  security_headers_config {
    content_security_policy {
      content_security_policy = join("; ", [
        "default-src 'self'",
        "script-src 'self'",
        "style-src 'self' 'unsafe-inline'",
        "img-src 'self' https: data:",
        "connect-src 'self' ${aws_apigatewayv2_api.api.api_endpoint}",
        "frame-ancestors 'none'",
        "base-uri 'none'",
      ])
      override = true
    }
    content_type_options {
      override = true
    }
    referrer_policy {
      referrer_policy = "strict-origin-when-cross-origin"
      override        = true
    }
    strict_transport_security {
      access_control_max_age_sec = 31536000
      include_subdomains         = true
      override                   = true
    }
    frame_options {
      frame_option = "DENY"
      override     = true
    }
  }
}

# ---------- site hostname (optional) ----------

locals {
  custom_domain = var.site_domain != ""
  site_domains  = compact([aws_cloudfront_distribution.site.domain_name, var.site_domain])
}

data "aws_route53_zone" "site" {
  count        = local.custom_domain ? 1 : 0
  name         = var.site_zone
  private_zone = false
}

# CloudFront only uses certificates from us-east-1.
resource "aws_acm_certificate" "site" {
  count             = local.custom_domain ? 1 : 0
  provider          = aws.us_east_1
  domain_name       = var.site_domain
  validation_method = "DNS"
  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_route53_record" "site_validation" {
  for_each = local.custom_domain ? {
    for o in aws_acm_certificate.site[0].domain_validation_options : o.domain_name => o
  } : {}
  zone_id = data.aws_route53_zone.site[0].zone_id
  name    = each.value.resource_record_name
  type    = each.value.resource_record_type
  records = [each.value.resource_record_value]
  ttl     = 300
}

resource "aws_acm_certificate_validation" "site" {
  count                   = local.custom_domain ? 1 : 0
  provider                = aws.us_east_1
  certificate_arn         = aws_acm_certificate.site[0].arn
  validation_record_fqdns = [for r in aws_route53_record.site_validation : r.fqdn]
}

resource "aws_route53_record" "site" {
  for_each = local.custom_domain ? toset(["A", "AAAA"]) : toset([])
  zone_id  = data.aws_route53_zone.site[0].zone_id
  name     = var.site_domain
  type     = each.value
  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}

resource "aws_cloudfront_distribution" "site" {
  enabled             = true
  aliases             = local.custom_domain ? [var.site_domain] : []
  is_ipv6_enabled     = true
  default_root_object = "index.html"
  price_class         = "PriceClass_100"
  http_version        = "http2and3"

  origin {
    origin_id                = "site"
    domain_name              = aws_s3_bucket.site.bucket_regional_domain_name
    origin_access_control_id = aws_cloudfront_origin_access_control.site.id
  }

  default_cache_behavior {
    target_origin_id           = "site"
    viewer_protocol_policy     = "redirect-to-https"
    allowed_methods            = ["GET", "HEAD"]
    cached_methods             = ["GET", "HEAD"]
    compress                   = true
    cache_policy_id            = "658327ea-f89d-4fab-a63d-7e88639e58f6" # Managed-CachingOptimized
    response_headers_policy_id = aws_cloudfront_response_headers_policy.site.id
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    cloudfront_default_certificate = !local.custom_domain
    acm_certificate_arn            = local.custom_domain ? aws_acm_certificate_validation.site[0].certificate_arn : null
    ssl_support_method             = local.custom_domain ? "sni-only" : null
    minimum_protocol_version       = local.custom_domain ? "TLSv1.2_2021" : null
  }
}

data "aws_iam_policy_document" "site" {
  statement {
    actions   = ["s3:GetObject"]
    resources = ["${aws_s3_bucket.site.arn}/*"]
    principals {
      type        = "Service"
      identifiers = ["cloudfront.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "AWS:SourceArn"
      values   = [aws_cloudfront_distribution.site.arn]
    }
  }
}

resource "aws_s3_bucket_policy" "site" {
  bucket = aws_s3_bucket.site.id
  policy = data.aws_iam_policy_document.site.json
}
