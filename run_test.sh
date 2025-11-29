#!/bin/bash
# 运行 complete_client 示例测试脚本

set -e

cd "$(dirname "$0")"

echo "=========================================="
echo "开始运行 Flare IM SDK 完整功能示例测试"
echo "=========================================="
echo ""

# 设置日志级别
export RUST_LOG=info

# 运行示例（设置超时，避免无限等待）
echo "正在编译和运行示例..."
echo ""

# 使用 timeout 或者直接运行（如果系统支持）
if command -v gtimeout &> /dev/null; then
    # macOS 上可能需要安装 coreutils: brew install coreutils
    gtimeout 15 cargo run --example complete_client 2>&1 || true
elif command -v timeout &> /dev/null; then
    timeout 15 cargo run --example complete_client 2>&1 || true
else
    # 如果没有 timeout 命令，直接运行并手动中断
    echo "注意: 程序将运行15秒后自动停止（如果没有服务器连接）"
    cargo run --example complete_client 2>&1 &
    PID=$!
    sleep 15
    kill $PID 2>/dev/null || true
    wait $PID 2>/dev/null || true
fi

echo ""
echo "=========================================="
echo "测试完成"
echo "=========================================="

