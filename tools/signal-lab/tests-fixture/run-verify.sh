#!/bin/bash
# 模拟验证：scale=3.0、42 游戏秒周期（sim 时钟驱动）
# 期望：相邻事件实测倍率 = 3.000 → "simulation 时钟驱动（模拟秒）"
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
ANALYZE="$DIR/../SignalLabAnalyze/bin/Debug/net9.0/SignalLabAnalyze"
OUT=$("$ANALYZE" "$DIR/samples.csv" "$DIR/events.csv")
echo "$OUT" | grep -q "3.000.*3.000.*simulation" && echo "PASS: TL-01 判定正确" || { echo "FAIL: TL-01 判定异常"; echo "$OUT"; exit 1; }
