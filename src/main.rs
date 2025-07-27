use bitcoin_debugger::*;
use clap::{Arg, Command};
use serde_json;
use std::convert::Infallible;
use warp::Filter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let matches = Command::new("bitcoin-debugger")
        .version("1.0")
        .about("Bitcoin Metaprotocol Transaction Debugger")
        .arg(Arg::new("mode")
            .short('m')
            .long("mode")
            .value_name("MODE")
            .help("Run mode: cli or server")
            .default_value("server"))
        .arg(Arg::new("txid")
            .short('t')
            .long("txid")
            .value_name("TXID")
            .help("Transaction ID to debug (CLI mode)"))
        .arg(Arg::new("port")
            .short('p')
            .long("port")
            .value_name("PORT")
            .help("Server port")
            .default_value("8000"))
        .get_matches();

    let mode = matches.get_one::<String>("mode").unwrap();
    
    match mode.as_str() {
        "cli" => {
            if let Some(txid) = matches.get_one::<String>("txid") {
                run_cli(txid).await?;
            } else {
                println!("Error: --txid required in CLI mode");
                std::process::exit(1);
            }
        }
        "server" => {
            let port: u16 = matches.get_one::<String>("port").unwrap().parse()?;
            run_server(port).await?;
        }
        _ => {
            println!("Error: Invalid mode. Use 'cli' or 'server'");
            std::process::exit(1);
        }
    }
    
    Ok(())
}

async fn run_cli(txid: &str) -> anyhow::Result<()> {
    println!("🔍 Debugging transaction: {}", txid);
    
    match debug_transaction(txid).await {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
    
    Ok(())
}

async fn run_server(port: u16) -> anyhow::Result<()> {
    println!("🚀 Starting Bitcoin Metaprotocol Debugger on port {}", port);
    
    // CORS headers
    let cors = warp::cors()
        .allow_any_origin()
        .allow_headers(vec!["content-type"])
        .allow_methods(vec!["GET", "POST", "OPTIONS"]);
    
    // Debug endpoint
    let debug = warp::path!("api" / "debug" / String)
        .and(warp::post())
        .and_then(handle_debug);
    
    // Raw transaction endpoint
    let raw_tx = warp::path!("api" / "tx" / String)
        .and(warp::get())
        .and_then(handle_raw_tx);
    
    // Test endpoint
    let test = warp::path!("api" / "test")
        .and(warp::get())
        .and_then(handle_test);
    
    // Root endpoint
    let root = warp::path::end()
        .and(warp::get())
        .map(move || {
            warp::reply::json(&serde_json::json!({
                "message": "Bitcoin Metaprotocol Debugger",
                "endpoints": {
                    "debug": "POST /api/debug/:txid",
                    "raw": "GET /api/tx/:txid", 
                    "test": "GET /api/test"
                },
                "example": format!("curl -X POST http://localhost:{}/api/debug/b61b0172d95e266c18aea0c624db987e971a5d6d4ebc2aaed85da4642d635735", port)
            }))
        });
    
    let routes = debug
        .or(raw_tx)
        .or(test)
        .or(root)
        .with(cors);
    
    println!("Ready! Try:");
    println!("  curl -X POST http://localhost:{}/api/debug/b61b0172d95e266c18aea0c624db987e971a5d6d4ebc2aaed85da4642d635735", port);
    
    warp::serve(routes)
        .run(([0, 0, 0, 0], port))
        .await;
    
    Ok(())
}

async fn handle_debug(txid: String) -> Result<impl warp::Reply, Infallible> {
    // Validate txid format
    if txid.len() != 64 || !txid.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Invalid transaction ID"})),
            warp::http::StatusCode::BAD_REQUEST,
        ));
    }
    
    match debug_transaction(&txid).await {
        Ok(result) => Ok(warp::reply::with_status(
            warp::reply::json(&result),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": e.to_string()})),
            warp::http::StatusCode::NOT_FOUND,
        )),
    }
}

async fn handle_raw_tx(txid: String) -> Result<impl warp::Reply, Infallible> {
    let client = BitcoinClient::new();
    
    match client.get_transaction(&txid).await {
        Ok(tx) => Ok(warp::reply::with_status(
            warp::reply::json(&tx),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": e.to_string()})),
            warp::http::StatusCode::NOT_FOUND,
        )),
    }
}

async fn handle_test() -> Result<impl warp::Reply, Infallible> {
    let test_txs = vec![
        "b61b0172d95e266c18aea0c624db987e971a5d6d4ebc2aaed85da4642d635735", // ORDI deploy
        "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b", // Regular BTC tx
    ];
    
    let mut results = Vec::new();
    
    for txid in test_txs {
        match debug_transaction(txid).await {
            Ok(result) => {
                results.push(serde_json::json!({
                    "txid": txid,
                    "activities": result.activities.len(),
                    "protocols": result.protocols_detected,
                    "operations": result.summary.operations
                }));
            }
            Err(e) => {
                results.push(serde_json::json!({
                    "txid": txid,
                    "error": e.to_string()
                }));
            }
        }
    }
    
    Ok(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({"test_results": results})),
        warp::http::StatusCode::OK,
    ))
}