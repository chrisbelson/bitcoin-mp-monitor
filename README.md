# Bitcoin Metaprotocol Monitor

Real-time monitoring for Bitcoin metaprotocols. Watch BRC-20, Runes, and Stamps transactions as they happen, with a live dashboard and WebSocket streaming.

---

## What It Does

- Monitors Bitcoin mempool and recent blocks in real-time
- Detects metaprotocol activity (BRC-20, Runes, Stamps)
- Live WebSocket feed of protocol transactions
- Beautiful dashboard with activity charts
- REST API for analyzing specific transactions

---

## Usage

```bash
# Run in demo mode
cargo run -- --demo

# Run with real blockchain data (watch out for rate limits)
cargo run

# Run demo script
./demo.sh
```

## API Examples

**ORDI deploy:**
```bash
curl -X POST localhost:8000/api/analyze/b61b0172d95e266c18aea0c624db987e971a5d6d4ebc2aaed85da4642d635735
```

**Runes transaction:**
```bash
curl -X POST localhost:8000/api/analyze/2bb85f4b004be6da54f766c17c1e855187327112c231ef2ff35ebad0ea67c69e
```

**Protocol stats:**
```bash
curl localhost:8000/api/stats
```

---

## How It Works

The monitor runs three parallel tasks:

1. **Mempool Scanner** - checks unconfirmed transactions every 60s
2. **Block Scanner** - checks recent blocks every 5 minutes  
3. **Protocol Parsers** - detect activity in witness/output data

Different protocols store data in different places:
- **BRC-20**: Ordinals inscriptions in witness data
- **Runes**: OP_RETURN outputs with specific markers
- **Stamps**: Embedded in outputs with Stamps prefix

```rust
// scan all protocols in one pass
let activities = vec![
    parse_brc20(&tx),    // check witness
    parse_runes(&tx),    // check op_return
    parse_stamps(&tx),   // check outputs
];
```

---

## Demo Mode

Due to Blockstream API rate limits (700 req/hour), use demo mode for hackathon:

```bash
cargo run -- --demo
```

This generates simulate4d transactions showing all features without hitting external APIs.

---

## Why I Built This

There was a point where the fragmented indexer problem had been mentioned so I wanted to build something to show the full picture of what's going on.

Also wanted real-time monitoring, not just one-off debugging. Seeing transactions flow gives you a feel for what's actually happening.

---

## Hackathon Notes

Built for pleb.fi Miami 2025. Spent most time on protocol parsers, then added the dashboard for better demos.

The live feed is pretty cool when it catches real metaprotocol activity.

Has a lot of rough edges but shows the concept.