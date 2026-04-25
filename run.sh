#!/bin/bash
cd "$(dirname "$0")"
cargo run --release 2>&1
