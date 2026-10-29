// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! One small structured request through whichever provider the environment selects:
//! `CATALOG_CLAUDE_PROVIDER=bedrock cargo run -p catalog-claude --example ping [text]`.
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let claude =
        catalog_claude::Claude::from_env("PING_MODEL", "PING_EFFORT", Duration::from_secs(30))
            .await?
            .ok_or_else(|| anyhow::anyhow!("no Claude provider configured"))?;
    let text = std::env::args().nth(1).unwrap_or_else(|| "Say hi".into());
    let schema = serde_json::json!({
        "type": "object", "additionalProperties": false,
        "required": ["reply"], "properties": { "reply": { "type": "string" } },
    });
    let a = claude
        .structured("Reply briefly.", &text, schema, 1024)
        .await?;
    println!("{} {}", a.model, a.value);
    Ok(())
}
