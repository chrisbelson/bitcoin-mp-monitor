use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::broadcast;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub txid: String,
    pub size: u32,
    pub fee: Option<u64>,
    pub status: TxStatus,
    pub vout: Vec<Output>,
    pub vin: Vec<Input>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Input {
    pub txid: String,
    pub vout: u32,
    pub witness: Option<Vec<String>>,
    pub prevout: Option<Output>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxStatus {
    pub confirmed: bool,
    #[serde(default)]
    pub block_height: Option<u32>,
    #[serde(default)]
    pub block_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub scriptpubkey: String,
    pub scriptpubkey_address: Option<String>,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub protocol: String,
    pub operation: String,
    pub output: usize,
    pub data: HashMap<String, serde_json::Value>,
    pub changes: Vec<StateChange>,
    pub description: String,
    pub value_usd: Option<f64>,
    pub importance: u8,
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
pub struct ProtocolStats {
    pub protocol: String,
    pub total_txs: u64,
    pub total_volume: u64,
    pub active_tokens: u32,
    pub last_activity: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTransaction {
    pub txid: String,
    pub timestamp: u64,
    pub protocols: Vec<String>,
    pub total_value: u64,
    pub activities: Vec<Activity>,
    pub fee_rate: f64,
    pub size: u32,
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

    pub async fn get_mempool_txs(&self) -> anyhow::Result<Vec<String>> {
        let url = format!("{}/mempool/recent", self.base_url);
        let resp = self.client.get(&url).send().await?;
        
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        
        let recent_txs: Vec<serde_json::Value> = resp.json().await?;
        let txids: Vec<String> = recent_txs
            .into_iter()
            .filter_map(|tx| tx.get("txid").and_then(|t| t.as_str()).map(String::from))
            .take(5)
            .collect();
        
        Ok(txids)
    }

    pub async fn get_recent_blocks(&self) -> anyhow::Result<Vec<String>> {
        let url = format!("{}/blocks", self.base_url);
        let resp = self.client.get(&url).send().await?;
        
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        
        let blocks: Vec<serde_json::Value> = resp.json().await?;
        
        let mut all_txids = Vec::new();
        
        if let Some(block) = blocks.first() {
            if let Some(hash) = block.get("id").and_then(|h| h.as_str()) {
                let txs_url = format!("{}/block/{}/txs", self.base_url, hash);
                if let Ok(resp) = self.client.get(&txs_url).send().await {
                    if let Ok(txs) = resp.json::<Vec<serde_json::Value>>().await {
                        let txids: Vec<String> = txs
                            .into_iter()
                            .filter_map(|tx| tx.get("txid").and_then(|t| t.as_str()).map(String::from))
                            .take(10)
                            .collect();
                        all_txids.extend(txids);
                    }
                }
            }
        }
        
        Ok(all_txids)
    }
}

// Protocol Parsers
pub mod parsers {
    use super::*;
    use regex::Regex;
    
    pub fn parse_brc20(tx: &Transaction) -> Vec<Activity> {
        let mut activities = Vec::new();
        
        for (idx, input) in tx.vin.iter().enumerate() {
            if let Some(witness) = &input.witness {
                if let Some(activity) = extract_brc20_from_witness(witness, idx) {
                    activities.push(activity);
                }
            }
        }
        
        activities
    }
    
    pub fn parse_stamps(tx: &Transaction) -> Vec<Activity> {
        let mut activities = Vec::new();
        
        for (idx, out) in tx.vout.iter().enumerate() {
            if let Some(activity) = extract_stamps_from_output(out, idx) {
                activities.push(activity);
            }
        }
        
        activities
    }
    
    pub fn parse_runes(tx: &Transaction) -> Vec<Activity> {
        let mut activities = Vec::new();
        
        for (idx, out) in tx.vout.iter().enumerate() {
            if out.scriptpubkey.starts_with("6a5d") {
                if let Some(activity) = extract_runes_from_output(out, idx) {
                    activities.push(activity);
                }
            }
        }
        
        activities
    }
    
    fn extract_brc20_from_witness(witness: &[String], idx: usize) -> Option<Activity> {
        for witness_item in witness {
            let bytes = hex::decode(witness_item).ok()?;
            let hex_str = hex::encode(&bytes);
            
            if !hex_str.contains("6f7264") {
                continue;
            }
            
            let content_start = hex_str.find("6f7264")?;
            let content_hex = &hex_str[content_start + 20..];
            
            let content_bytes = hex::decode(content_hex).ok()?;
            let content_text = String::from_utf8_lossy(&content_bytes).replace('\0', "");
            
            let json_pattern = Regex::new(r#"\{[^}]*"p"\s*:\s*"brc-20"[^}]*\}"#).ok()?;
            if let Some(json_match) = json_pattern.find(&content_text) {
                if let Ok(brc20_data) = serde_json::from_str::<serde_json::Value>(json_match.as_str()) {
                    return parse_brc20_json(&brc20_data, idx);
                }
            }
        }
        None
    }
    
    fn parse_brc20_json(brc20_data: &serde_json::Value, idx: usize) -> Option<Activity> {
        let op = brc20_data.get("op")?.as_str()?.to_lowercase();
        let tick = brc20_data.get("tick")?.as_str()?.to_uppercase();
        
        let mut data = HashMap::new();
        data.insert("tick".to_string(), serde_json::Value::String(tick.clone()));
        data.insert("operation".to_string(), serde_json::Value::String(op.clone()));
        
        if let Some(amt) = brc20_data.get("amt") {
            data.insert("amount".to_string(), amt.clone());
        }
        if let Some(max) = brc20_data.get("max") {
            data.insert("max_supply".to_string(), max.clone());
        }
        if let Some(lim) = brc20_data.get("lim") {
            data.insert("limit".to_string(), lim.clone());
        }
        
        let importance = match op.as_str() {
            "deploy" => 8,
            "mint" => 5,
            "transfer" => 3,
            _ => 1,
        };
        
        let description = match op.as_str() {
            "deploy" => format!("New BRC-20 token '{}' deployed", tick),
            "mint" => format!("Minted {} tokens", tick),
            "transfer" => format!("Transfer {} tokens", tick),
            _ => format!("Unknown {} operation", op),
        };
        
        Some(Activity {
            protocol: "brc20".to_string(),
            operation: op,
            output: idx,
            data,
            changes: vec![],
            description,
            value_usd: None,
            importance,
        })
    }
    
    fn extract_stamps_from_output(out: &Output, idx: usize) -> Option<Activity> {
        if out.scriptpubkey.contains("534d505300") {
            let mut data = HashMap::new();
            data.insert("protocol".to_string(), serde_json::Value::String("stamps".to_string()));
            
            return Some(Activity {
                protocol: "stamps".to_string(),
                operation: "mint".to_string(),
                output: idx,
                data,
                changes: vec![],
                description: "STAMPS token activity detected".to_string(),
                value_usd: None,
                importance: 6,
            });
        }
        None
    }
    
    fn extract_runes_from_output(_out: &Output, idx: usize) -> Option<Activity> {
        let mut data = HashMap::new();
        data.insert("protocol".to_string(), serde_json::Value::String("runes".to_string()));
        
        Some(Activity {
            protocol: "runes".to_string(),
            operation: "transfer".to_string(),
            output: idx,
            data,
            changes: vec![],
            description: "Runes protocol activity".to_string(),
            value_usd: None,
            importance: 7,
        })
    }
}

// Live monitoring system
pub struct MetaprotocolMonitor {
    client: BitcoinClient,
    tx_broadcaster: broadcast::Sender<LiveTransaction>,
    stats: Arc<RwLock<HashMap<String, ProtocolStats>>>,
}

impl MetaprotocolMonitor {
    pub fn new() -> (Self, broadcast::Receiver<LiveTransaction>) {
        let (tx, rx) = broadcast::channel(1000);
        
        (Self {
            client: BitcoinClient::new(),
            tx_broadcaster: tx,
            stats: Arc::new(RwLock::new(HashMap::new())),
        }, rx)
    }
    
    pub async fn start_monitoring(self: Arc<Self>, demo_mode: bool) {
        if demo_mode {
            let demo_monitor = self.clone();
            tokio::spawn(async move {
                println!("Demo mode active - generating simulated transactions...");
                let mut rng = 0u64;
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2 + (rng % 3))).await;
                    
                    let num_txs = 1 + (rng % 3) as usize;
                    for _ in 0..num_txs {
                        demo_monitor.generate_demo_transaction(rng).await;
                        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                    }
                }
            });
        } else {
            let mempool_monitor = self.clone();
            tokio::spawn(async move {
                loop {
                    if let Err(e) = mempool_monitor.scan_mempool().await {
                        eprintln!("Mempool scan error: {}", e);
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                }
            });
            
            let block_monitor = self.clone();
            tokio::spawn(async move {
                loop {
                    if let Err(e) = block_monitor.scan_recent_blocks().await {
                        eprintln!("Block scan error: {}", e);
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
                }
            });
        }
    }
    
    async fn generate_demo_transaction(&self, seed: u64) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        let hash = hasher.finish();
        
        let protocols = ["brc20", "runes", "stamps"];
        let protocol = protocols[(hash % 3) as usize];
        
        let activity = match protocol {
            "brc20" => {
                let ops = ["deploy", "mint", "transfer"];
                let op = ops[(hash / 3 % 3) as usize];
                let tickers = ["ORDI", "SATS", "MEME", "PEPE", "WZRD", "BITS"];
                let tick = tickers[(hash / 9 % 6) as usize];
                
                let mut data = HashMap::new();
                data.insert("tick".to_string(), serde_json::Value::String(tick.to_string()));
                data.insert("operation".to_string(), serde_json::Value::String(op.to_string()));
                
                if op != "deploy" {
                    let amount = 1000 + (hash % 9000);
                    data.insert("amount".to_string(), serde_json::Value::Number(amount.into()));
                }
                
                let description = match op {
                    "deploy" => format!("New BRC-20 token '{}' deployed", tick),
                    "mint" => format!("Minted {} {} tokens", 1000 + (hash % 9000), tick),
                    "transfer" => format!("Transfer {} {} tokens", 100 + (hash % 900), tick),
                    _ => "Activity".to_string(),
                };
                
                Activity {
                    protocol: "brc20".to_string(),
                    operation: op.to_string(),
                    output: 0,
                    data,
                    changes: vec![],
                    description,
                    value_usd: None,
                    importance: match op { "deploy" => 8, "mint" => 5, _ => 3 },
                }
            },
            "runes" => {
                let rune_names = ["SATOSHI.NAKAMOTO", "BITCOIN.PIZZA", "GENESIS.BLOCK"];
                let rune = rune_names[(hash / 3 % 3) as usize];
                
                let mut data = HashMap::new();
                data.insert("rune".to_string(), serde_json::Value::String(rune.to_string()));
                data.insert("amount".to_string(), serde_json::Value::Number((10000 + hash % 90000).into()));
                
                Activity {
                    protocol: "runes".to_string(),
                    operation: "transfer".to_string(),
                    output: 0,
                    data,
                    changes: vec![],
                    description: format!("{} Runes transferred", rune),
                    value_usd: None,
                    importance: 7,
                }
            },
            _ => Activity {
                protocol: "stamps".to_string(),
                operation: "mint".to_string(),
                output: 0,
                data: HashMap::new(),
                changes: vec![],
                description: format!("STAMP #{} minted", 1000 + (hash % 9000)),
                value_usd: None,
                importance: 6,
            },
        };
        
        let txid = format!("{:016x}{:016x}{:016x}{:016x}", hash, hash.rotate_left(16), hash.rotate_left(32), hash.rotate_left(48));
        
        let live_tx = LiveTransaction {
            txid,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            protocols: vec![protocol.to_string()],
            total_value: 10000 + (hash % 990000),
            activities: vec![activity],
            fee_rate: 10.0 + (hash % 140) as f64,
            size: 200 + (hash % 800) as u32,
        };
        
        self.update_stats(&live_tx).await;
        let _ = self.tx_broadcaster.send(live_tx);
    }
    
    async fn scan_mempool(&self) -> anyhow::Result<()> {
        let txids = self.client.get_mempool_txs().await?;
        println!("Scanning {} mempool transactions...", txids.len());
        
        for txid in txids {
            if let Ok(tx) = self.client.get_transaction(&txid).await {
                self.process_transaction(tx).await;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
        
        Ok(())
    }
    
    async fn scan_recent_blocks(&self) -> anyhow::Result<()> {
        let txids = self.client.get_recent_blocks().await?;
        println!("Scanning {} block transactions...", txids.len());
        
        for txid in txids {
            if let Ok(tx) = self.client.get_transaction(&txid).await {
                self.process_transaction(tx).await;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
        
        Ok(())
    }
    
    async fn process_transaction(&self, tx: Transaction) {
        let mut all_activities = Vec::new();
        let mut protocols = Vec::new();
        
        let brc20 = parsers::parse_brc20(&tx);
        if !brc20.is_empty() {
            protocols.push("brc20".to_string());
            all_activities.extend(brc20);
        }
        
        let stamps = parsers::parse_stamps(&tx);
        if !stamps.is_empty() {
            protocols.push("stamps".to_string());
            all_activities.extend(stamps);
        }
        
        let runes = parsers::parse_runes(&tx);
        if !runes.is_empty() {
            protocols.push("runes".to_string());
            all_activities.extend(runes);
        }
        
        if !all_activities.is_empty() {
            println!("Found {} protocol(s) in tx {}: {:?}", 
                protocols.len(), &tx.txid[..8], protocols);
            
            let total_value: u64 = tx.vout.iter().map(|o| o.value).sum();
            let fee_rate = tx.fee.unwrap_or(0) as f64 / tx.size as f64;
            
            let live_tx = LiveTransaction {
                txid: tx.txid.clone(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                protocols,
                total_value,
                activities: all_activities,
                fee_rate,
                size: tx.size,
            };
            
            self.update_stats(&live_tx).await;
            let _ = self.tx_broadcaster.send(live_tx);
        }
    }
    
    async fn update_stats(&self, tx: &LiveTransaction) {
        let mut stats = self.stats.write().await;
        
        for protocol in &tx.protocols {
            let stat = stats.entry(protocol.clone()).or_insert(ProtocolStats {
                protocol: protocol.clone(),
                total_txs: 0,
                total_volume: 0,
                active_tokens: 0,
                last_activity: 0,
            });
            
            stat.total_txs += 1;
            stat.total_volume += tx.total_value;
            stat.last_activity = tx.timestamp;
        }
    }
    
    pub async fn get_stats(&self) -> HashMap<String, ProtocolStats> {
        self.stats.read().await.clone()
    }
}

// Analysis functions
pub async fn analyze_transaction(txid: &str) -> anyhow::Result<serde_json::Value> {
    let client = BitcoinClient::new();
    let tx = client.get_transaction(txid).await?;
    
    let mut activities = Vec::new();
    let mut protocols = Vec::new();
    
    let brc20 = parsers::parse_brc20(&tx);
    if !brc20.is_empty() {
        protocols.push("brc20");
        activities.extend(brc20);
    }
    
    let stamps = parsers::parse_stamps(&tx);
    if !stamps.is_empty() {
        protocols.push("stamps");
        activities.extend(stamps);
    }
    
    let runes = parsers::parse_runes(&tx);
    if !runes.is_empty() {
        protocols.push("runes");
        activities.extend(runes);
    }
    
    let total_value: u64 = tx.vout.iter().map(|o| o.value).sum();
    let fee_rate = tx.fee.unwrap_or(0) as f64 / tx.size as f64;
    
    Ok(serde_json::json!({
        "txid": txid,
        "size": tx.size,
        "fee": tx.fee,
        "fee_rate_sat_vb": fee_rate,
        "total_value_sats": total_value,
        "total_value_btc": total_value as f64 / 100_000_000.0,
        "protocols_detected": protocols,
        "activities": activities,
        "activity_count": activities.len(),
        "is_metaprotocol": !activities.is_empty(),
        "importance_score": activities.iter().map(|a| a.importance).max().unwrap_or(0),
        "timestamp": tx.status.block_time,
        "confirmed": tx.status.confirmed,
        "block_height": tx.status.block_height,
    }))
}