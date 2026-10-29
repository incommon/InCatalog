# TODOs

## Rename the AWS resources from `catabasis` to `incatalog`

The code and docs use `incatalog`, but the deployed AWS resources still carry the old
working name. These must change together, as a planned migration:

- **`var.name` default** in `infra/terraform/variables.tf`. Every resource is named from
  it (ECR repo, both Lambdas and their log groups, the S3 data and site buckets, CloudFront,
  the EventBridge rule, API Gateway). Changing it makes `terraform apply` destroy and
  recreate them, so copy the data and site buckets' contents and push the image to the new
  ECR repo first (or `terraform state mv` / import where a resource can keep its identity).
- **The state bucket** `catabasis-tfstate-<account>` and key `catabasis/terraform.tfstate`
  in `infra/terraform/backend.hcl` and `infra/state-bucket.sh`. S3 buckets can't be
  renamed: create the new bucket, update both files, run
  `terraform init -backend-config=backend.hcl -migrate-state`, then delete the old bucket.
- **The GitHub `dev` environment's variables.** `ECR_REPOSITORY` names the ECR repository
  and `AWS_ROLE_ARN` the push role (`<name>-github-image-push`); both change with
  `var.name`, so update them from the new Terraform outputs.
- **The `catabasis-*` ARNs** in `infra/deploy-policy.json`. Update the policy in IAM
  wherever it is attached, before the first deploy under the new names, or deploys lose
  permission.

## Search by more groupings: "Schengen", "NATO", "OECD", research networks, …

Region search (`region=`, `catalog-core/src/regions.rs`) covers UN M49 geographic regions,
Asia-Pacific, the EU, the EEA, and BRICS. Extend it:

- **Further political, security and economic groupings,** each with its source and a
  dated membership list:
  - *Treaty and travel areas:* the Schengen Area, which is not the EU (it has non-EU
    members such as Norway and Switzerland, and EU states outside it), the eurozone, and
    the Nordic Council.
  - *Security alliances:* NATO, and Five Eyes.
  - *Economic and trade blocs:* the OECD, G7, G20, ASEAN, APEC, Mercosur, USMCA, the
    African Union, the Gulf Cooperation Council, and the Commonwealth.
  - *Research networks,* where people search that way: the countries of GÉANT's NRENs,
    and of the regional networks RedCLARA, APAN, WACREN, UbuntuNet and ASREN.
- **Keep the data maintainable.** Store groupings as data (say
  `crates/catalog-core/data/regions.json`), each with its source, the date its list was
  checked, and per-country join dates where they matter, rather than hard-coding them.
  Membership moves (Sweden and Finland joining NATO, Bulgaria and Romania joining
  Schengen), so a test should fail once a list is older than a year, as a prompt to
  recheck it.
- **Deliberately leave out** groupings whose membership is contested or politically
  sensitive to assert, and anything that labels countries by a judgment, such as
  sanctions lists. If one is added later, document how its list was chosen.
- **Natural-language search.** Teach the rules and the Claude reading the new names and
  their common forms ("Schengen countries", "NATO members", "OECD").
