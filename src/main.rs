use bitcoin_monitor::*;
use clap::{Arg, Command};
use std::sync::Arc;
use warp::Filter;
use warp::ws::{Message, WebSocket};
use futures_util::{StreamExt, SinkExt};
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let matches = Command::new("bitcoin-monitor")
        .version("2.0")
        .about("Real-Time Bitcoin Metaprotocol Monitor - Track BRC-20, Runes, Stamps & More")
        .arg(Arg::new("port")
            .short('p')
            .long("port")
            .value_name("PORT")
            .help("Server port")
            .default_value("8000"))
        .arg(Arg::new("demo")
            .short('d')
            .long("demo")
            .help("Enable demo mode with simulated transactions")
            .action(clap::ArgAction::SetTrue))
        .get_matches();

    let port: u16 = matches.get_one::<String>("port").unwrap().parse()?;
    let demo_mode = matches.get_flag("demo");
    
    println!("Bitcoin Metaprotocol Monitor v2.0");
    if demo_mode {
        println!("Starting in DEMO MODE - simulated transactions enabled");
    }
    println!("Starting real-time monitoring...");
    
    // Initialize monitor
    let (monitor, tx_receiver) = MetaprotocolMonitor::new();
    let monitor = Arc::new(monitor);
    
    // Start monitoring
    monitor.clone().start_monitoring(demo_mode).await;
    
    // Setup routes
    let routes = setup_routes(monitor.clone(), tx_receiver);
    
    println!("\nServer ready on http://localhost:{}", port);
    println!("Dashboard: http://localhost:{}/", port);
    println!("WebSocket: ws://localhost:{}/ws", port);
    println!("API: http://localhost:{}/api/analyze/{{txid}}", port);
    println!("\nMonitoring protocols: BRC-20, Runes, Stamps");
    
    warp::serve(routes)
        .run(([0, 0, 0, 0], port))
        .await;
    
    Ok(())
}

fn setup_routes(
    monitor: Arc<MetaprotocolMonitor>,
    tx_receiver: broadcast::Receiver<LiveTransaction>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let cors = warp::cors()
        .allow_any_origin()
        .allow_headers(vec!["content-type"])
        .allow_methods(vec!["GET", "POST", "OPTIONS"]);
    
    // Dashboard HTML
    let dashboard = warp::path::end()
        .and(warp::get())
        .map(|| {
            let html = include_str!("../static/dashboard.html");
            warp::reply::html(html)
        });
    
    // WebSocket endpoint
    let tx_receiver = Arc::new(tokio::sync::Mutex::new(tx_receiver));
    let ws = warp::path("ws")
        .and(warp::ws())
        .and(with_monitor(tx_receiver))
        .map(|ws: warp::ws::Ws, tx_rx| {
            ws.on_upgrade(move |socket| websocket_handler(socket, tx_rx))
        });
    
    // API endpoints
    let api_analyze = warp::path!("api" / "analyze" / String)
        .and(warp::post())
        .and_then(handle_analyze);
    
    let monitor_stats = monitor.clone();
    let api_stats = warp::path!("api" / "stats")
        .and(warp::get())
        .and(with_monitor_stats(monitor_stats))
        .and_then(handle_stats);
    
    let api_health = warp::path!("api" / "health")
        .and(warp::get())
        .map(|| warp::reply::json(&serde_json::json!({
            "status": "healthy",
            "version": "2.0",
            "protocols": ["brc20", "runes", "stamps"]
        })));
    
    dashboard
        .or(ws)
        .or(api_analyze)
        .or(api_stats)
        .or(api_health)
        .with(cors)
}

fn with_monitor(
    tx_rx: Arc<tokio::sync::Mutex<broadcast::Receiver<LiveTransaction>>>
) -> impl Filter<Extract = (Arc<tokio::sync::Mutex<broadcast::Receiver<LiveTransaction>>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || tx_rx.clone())
}

fn with_monitor_stats(
    monitor: Arc<MetaprotocolMonitor>
) -> impl Filter<Extract = (Arc<MetaprotocolMonitor>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || monitor.clone())
}

async fn websocket_handler(
    ws: WebSocket,
    tx_rx: Arc<tokio::sync::Mutex<broadcast::Receiver<LiveTransaction>>>,
) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    
    // Clone receiver for this connection
    let mut rx = {
        let mut locked = tx_rx.lock().await;
        locked.resubscribe()
    };
    
    // Send transactions to websocket
    let send_task = tokio::spawn(async move {
        while let Ok(tx) = rx.recv().await {
            let msg = serde_json::to_string(&tx).unwrap();
            if ws_tx.send(Message::text(msg)).await.is_err() {
                break;
            }
        }
    });
    
    // Handle incoming messages (if any)
    while let Some(Ok(_msg)) = ws_rx.next().await {
    }
    
    send_task.abort();
}

async fn handle_analyze(txid: String) -> Result<impl warp::Reply, warp::Rejection> {
    match analyze_transaction(&txid).await {
        Ok(result) => Ok(warp::reply::json(&result)),
        Err(e) => Ok(warp::reply::json(&serde_json::json!({
            "error": e.to_string()
        }))),
    }
}

async fn handle_stats(monitor: Arc<MetaprotocolMonitor>) -> Result<impl warp::Reply, warp::Rejection> {
    let stats = monitor.get_stats().await;
    Ok(warp::reply::json(&stats))
}