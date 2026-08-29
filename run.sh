#!/bin/bash
export UPSTREAM_IP=127.0.0.1
export UPSTREAM_PORT=6060
export DEVICE_COUNT=30000
export BASE_PORT=10001
export DEVICE_ID_PREFIX=3402000000
export PASSWORD=123456
export ZLM_API_BASE=http://127.0.0.1:9080
export ZLM_SECRET==your_secret_key_here
export FIXED_STREAM=rtp/test_stream
export PUBLIC_IP=127.0.0.1
export HEARTBEAT_INTERVAL=60
export REGISTER_EXPIRES=3600
export RUST_LOG=warn

ulimit -n 655350
exec ./gbhub-stress
