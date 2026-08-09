#!/bin/bash
# analyze.sh <csv>：平均 FPS + 1% low（P99 帧时间）
f=$1
echo "== $f =="
tail -n +2 "$f" | cut -d, -f10 | awk '$1>0 && $1<500 {s+=$1; n++} END {printf "平均帧时间=%.3fms  平均FPS=%.1f\n", s/n, 1000*n/s}'
tail -n +2 "$f" | cut -d, -f10 | awk '$1>0 && $1<500' | sort -n > /tmp/ft.txt
n=$(wc -l < /tmp/ft.txt)
p99=$(sed -n "$((n * 99 / 100))p" /tmp/ft.txt)
p50=$(sed -n "$((n / 2))p" /tmp/ft.txt)
echo "样本=$n  P50=$(awk -v v=$p50 'BEGIN{printf "%.1f", 1000/v}')FPS  P99(1%low)=$(awk -v v=$p99 'BEGIN{printf "%.1f", 1000/v}')FPS"
