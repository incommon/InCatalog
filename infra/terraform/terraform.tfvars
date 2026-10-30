site_domain = "incatalog.texturedmilkproduct.casa"
site_zone   = "texturedmilkproduct.casa"

# Creates the role build-images.yml assumes to push images (main.tf, "CI image
# push"). Leaving this out would make the next apply delete that role.
github_oidc_provider_arn = "arn:aws:iam::719982589369:oidc-provider/token.actions.githubusercontent.com"
# The repository uses GitHub's immutable OIDC subjects, which name it by ID.
github_subject_prefix = "repo:incommon@294270554/InCatalog@1438674272"
