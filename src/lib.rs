use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub txid: String,
    pub size: u32,
    pub fee: Option<u64>,
    pub status: TxStatus,
    pub vout: Vec<Output>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxStatus {
    pub confirmed: bool,
    #[serde(default)]
    pub block_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub scriptpubkey: String,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    pub field: String,
    pub before: Option<String>,
    pub after: String,
    #[serde(rename = "type")]
    pub change_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub protocol: String,
    pub operation: String,
    pub output: usize,
    pub data: HashMap<String, serde_json::Value>,
    pub changes: Vec<StateChange>,
    pub description: String,
    pub raw_script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugResult {
    pub txid: String,
    pub size: u32,
    pub fee: Option<u64>,
    pub confirmations: u32,
    pub protocols_detected: Vec<String>,
    pub activities: Vec<Activity>,
    pub total_state_changes: usize,
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub total_activities: usize,
    pub protocols: Vec<String>,
    pub operations: Vec<String>,
}

pub struct BitcoinClient {
    client: reqwest::Client,
    base_url: String,
}

impl BitcoinClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://blockstream.info/api".to_string(),
        }
    }

    pub async fn get_transaction(&self, txid: &str) -> anyhow::Result<Transaction> {
        let url = format!("{}/tx/{}", self.base_url, txid);
        let resp = self.client.get(&url).send().await?;
        
        if !resp.status().is_success() {
            anyhow::bail!("Transaction not found");
        }
        
        let tx: Transaction = resp.json().await?;
        Ok(tx)
    }
}

pub fn parse_brc20(tx: &Transaction) -> Vec<Activity> {
    let mut activities = Vec::new();
    
    for (idx, out) in tx.vout.iter().enumerate() {
        if let Some(activity) = extract_brc20_from_output(out, idx) {
            activities.push(activity);
        }
    }
    
    activities
}

fn extract_brc20_from_output(out: &Output, idx: usize) -> Option<Activity> {
    let script = &out.scriptpubkey;
    
    // Must be OP_RETURN (starts with 6a)
    if !script.starts_with("6a") {
        return None;
    }
    
    // Extract the data after OP_RETURN
    let data_hex = if script.len() > 4 {
        &script[4..] // Skip 6a + length byte
    } else {
        return None;
    };
    
    // Convert hex to bytes
    let bytes = hex::decode(data_hex).ok()?;
    
    // Convert to string, removing null bytes
    let text = String::from_utf8_lossy(&bytes).replace('\0', "");
    
    // Look for BRC-20 JSON pattern
    let json_pattern = regex::Regex::new(r#"\{[^}]*"p"\s*:\s*"brc-20"[^}]*\}"#).ok()?;
    let json_match = json_pattern.find(&text)?;
    
    // Parse the JSON
    let brc20_data: serde_json::Value = serde_json::from_str(json_match.as_str()).ok()?;
    
    let op = brc20_data.get("op")?.as_str()?.to_lowercase();
    if !["deploy", "mint", "transfer"].contains(&op.as_str()) {
        return None;
    }
    
    let tick = brc20_data.get("tick")?.as_str()?.to_uppercase();
    let amt = brc20_data.get("amt").and_then(|v| v.as_str());
    let max = brc20_data.get("max").and_then(|v| v.as_str());
    let lim = brc20_data.get("lim").and_then(|v| v.as_str());
    
    // Build activity data
    let mut data = HashMap::new();
    data.insert("tick".to_string(), serde_json::Value::String(tick.clone()));
    data.insert("operation".to_string(), serde_json::Value::String(op.clone()));
    if let Some(a) = amt {
        data.insert("amount".to_string(), serde_json::Value::String(a.to_string()));
    }
    if let Some(m) = max {
        data.insert("max_supply".to_string(), serde_json::Value::String(m.to_string()));
    }
    if let Some(l) = lim {
        data.insert("limit".to_string(), serde_json::Value::String(l.to_string()));
    }
    
    // Calculate state changes
    let changes = match op.as_str() {
        "deploy" => {
            let mut changes = vec![StateChange {
                field: format!("token.{}.exists", tick),
                before: None,
                after: "true".to_string(),
                change_type: "created".to_string(),
            }];
            if let Some(m) = max {
                changes.push(StateChange {
                    field: format!("token.{}.max_supply", tick),
                    before: None,
                    after: m.to_string(),
                    change_type: "created".to_string(),
                });
            }
            changes
        }
        "mint" => {
            if let Some(a) = amt {
                vec![
                    StateChange {
                        field: format!("token.{}.total_supply", tick),
                        before: Some("prev_supply".to_string()),
                        after: format!("prev_supply + {}", a),
                        change_type: "updated".to_string(),
                    },
                    StateChange {
                        field: format!("balance.{}.minter", tick),
                        before: Some("prev_balance".to_string()),
                        after: format!("prev_balance + {}", a),
                        change_type: "updated".to_string(),
                    }
                ]
            } else {
                Vec::new()
            }
        }
        "transfer" => {
            if let Some(a) = amt {
                vec![
                    StateChange {
                        field: format!("balance.{}.sender", tick),
                        before: Some("sender_balance".to_string()),
                        after: format!("sender_balance - {}", a),
                        change_type: "updated".to_string(),
                    },
                    StateChange {
                        field: format!("balance.{}.receiver", tick),
                        before: Some("receiver_balance".to_string()),
                        after: format!("receiver_balance + {}", a),
                        change_type: "updated".to_string(),
                    }
                ]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };
    
    let description = match op.as_str() {
        "deploy" => format!("Deploy BRC-20 token '{}' with max supply {}", tick, max.unwrap_or("N/A")),
        "mint" => format!("Mint {} {} tokens", amt.unwrap_or("N/A"), tick),
        "transfer" => format!("Transfer {} {} tokens", amt.unwrap_or("N/A"), tick),
        _ => format!("Unknown {} operation", op),
    };
    
    Some(Activity {
        protocol: "brc20".to_string(),
        operation: op,
        output: idx,
        data,
        changes,
        description,
        raw_script: script.clone(),
    })
}

pub async fn debug_transaction(txid: &str) -> anyhow::Result<DebugResult> {
    let client = BitcoinClient::new();
    let tx = client.get_transaction(txid).await?;
    
    let brc20_activities = parse_brc20(&tx);
    let protocols: Vec<String> = if !brc20_activities.is_empty() {
        vec!["brc20".to_string()]
    } else {
        Vec::new()
    };
    
    let total_state_changes = brc20_activities.iter().map(|a| a.changes.len()).sum();
    let operations: Vec<String> = brc20_activities.iter()
        .map(|a| format!("{}:{}", a.protocol, a.operation))
        .collect();
    
    Ok(DebugResult {
        txid: txid.to_string(),
        size: tx.size,
        fee: tx.fee,
        confirmations: if tx.status.confirmed { 1 } else { 0 },
        protocols_detected: protocols.clone(),
        activities: brc20_activities.clone(),
        total_state_changes,
        summary: Summary {
            total_activities: brc20_activities.len(),
            protocols,
            operations,
        },
    })
}