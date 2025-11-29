#!/bin/bash

# 测试连接稳定性脚本

echo "🧪 开始测试连接稳定性..."
echo ""

# 清理之前的进程
pkill -f "two_clients_chat" 2>/dev/null
sleep 1

# 启动第一个客户端（user-alice）
echo "📱 启动客户端 1 (user-alice)..."
cd /Users/hg/workspace/flare/flare-im/flare-im-core-sdk
RUST_LOG=info MY_USER_ID=user-alice CHAT_WITH=user-bob cargo run --example two_clients_chat > /tmp/client1.log 2>&1 &
CLIENT1_PID=$!
echo "   客户端 1 PID: $CLIENT1_PID"

# 等待连接建立
sleep 3

# 检查客户端1是否还在运行
if ps -p $CLIENT1_PID > /dev/null 2>&1; then
    echo "✅ 客户端 1 仍在运行"
else
    echo "❌ 客户端 1 已退出"
    echo "   日志:"
    tail -20 /tmp/client1.log
    exit 1
fi

# 启动第二个客户端（user-bob）
echo "📱 启动客户端 2 (user-bob)..."
RUST_LOG=info MY_USER_ID=user-bob CHAT_WITH=user-alice cargo run --example two_clients_chat > /tmp/client2.log 2>&1 &
CLIENT2_PID=$!
echo "   客户端 2 PID: $CLIENT2_PID"

# 等待连接建立
sleep 3

# 检查两个客户端是否都在运行
if ps -p $CLIENT1_PID > /dev/null 2>&1 && ps -p $CLIENT2_PID > /dev/null 2>&1; then
    echo "✅ 两个客户端都在运行"
    echo ""
    echo "⏳ 等待 10 秒，检查连接稳定性..."
    sleep 10
    
    # 再次检查
    if ps -p $CLIENT1_PID > /dev/null 2>&1 && ps -p $CLIENT2_PID > /dev/null 2>&1; then
        echo "✅ 连接稳定！两个客户端都正常运行超过 10 秒"
        echo ""
        echo "📊 客户端 1 日志（最后 10 行）:"
        tail -10 /tmp/client1.log
        echo ""
        echo "📊 客户端 2 日志（最后 10 行）:"
        tail -10 /tmp/client2.log
        echo ""
        echo "✅ 测试通过！"
        
        # 清理
        kill $CLIENT1_PID $CLIENT2_PID 2>/dev/null
        exit 0
    else
        echo "❌ 连接不稳定，客户端已退出"
        if ! ps -p $CLIENT1_PID > /dev/null 2>&1; then
            echo "   客户端 1 已退出"
            tail -20 /tmp/client1.log
        fi
        if ! ps -p $CLIENT2_PID > /dev/null 2>&1; then
            echo "   客户端 2 已退出"
            tail -20 /tmp/client2.log
        fi
        exit 1
    fi
else
    echo "❌ 客户端启动失败"
    if ! ps -p $CLIENT1_PID > /dev/null 2>&1; then
        echo "   客户端 1 日志:"
        tail -20 /tmp/client1.log
    fi
    if ! ps -p $CLIENT2_PID > /dev/null 2>&1; then
        echo "   客户端 2 日志:"
        tail -20 /tmp/client2.log
    fi
    exit 1
fi

