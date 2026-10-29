// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

// Deployment overwrites this file with the API's URL (see infra/terraform).
// An empty apiBase means "same origin", which is how `make dev` serves it.
window.CATALOG_CONFIG = { apiBase: "" };
