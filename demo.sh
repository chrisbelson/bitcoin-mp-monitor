#!/bin/bash

echo "Bitcoin Metaprotocol Monitor"
echo "============================"

# Start server if not running
if ! curl -s http://localhost:8000/api/health > /dev/null 2>&1; then
    echo "Starting server..."
    cargo run -- --demo &
    SERVER_PID=$!

    echo -n "Waiting for server"
    for i in {1..30}; do
        if curl -s http://localhost:8000/api/health > /dev/null 2>&1; then
            echo " ready"
            break
        fi
        echo -n "."
        sleep 1
    done
    echo ""
fi

echo "Launching dashboard..."
open http://localhost:8000 2>/dev/null || xdg-open http://localhost:8000 2>/dev/null || echo "Visit: http://localhost:8000"

sleep 2

echo ""
echo "Analyzing transaction: ORDI deploy"
curl -s -X POST http://localhost:8000/api/analyze/b61b0172d95e266c18aea0c624db987e971a5d6d4ebc2aaed85da4642d635735 | jq .
echo ""

echo "Analyzing transaction: Runes"
curl -s -X POST http://localhost:8000/api/analyze/2bb85f4b004be6da54f766c17c1e855187327112c231ef2ff35ebad0ea67c69e | jq .
echo ""

echo "Protocol statistics"
curl -s http://localhost:8000/api/stats | jq .
echo ""

echo "Analyzing regular BTC transaction"
curl -s -X POST http://localhost:8000/api/analyze/4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b | jq .
echo ""

echo "Dashboard available at: http://localhost:8000"
echo ""

# Prompt to stop server if this script started it
if [ ! -z "$SERVER_PID" ]; then
    echo -n "Stop server? (y/n): "
    read -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Server continues (PID: $SERVER_PID). To stop: kill $SERVER_PID"
    else
        echo "Stopping server..."
        kill $SERVER_PID 2>/dev/null
    fi
fi

echo "Done."
