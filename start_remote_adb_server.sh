#!/bin/bash

# Remote ADB Server 启动脚本
# 自动检查和启动 Android 模拟器，然后启动 Remote ADB Server

set -e

# 显示帮助信息
show_help() {
    echo "Remote ADB Server 启动脚本"
    echo ""
    echo "用法: $0 [选项]"
    echo ""
    echo "选项:"
    echo "  -h, --help              显示此帮助信息"
    echo "  -p, --port PORT         指定服务器端口 (默认: 5555)"
    echo "  -b, --bind ADDRESS      指定绑定地址 (默认: 0.0.0.0)"
    echo "  -c, --config FILE       指定配置文件 (默认: config.toml)"
    echo "  -a, --avd NAME          指定要启动的 AVD 名称"
    echo "  -t, --timeout SECONDS   模拟器启动超时时间 (默认: 120)"
    echo "  --skip-emulator         跳过模拟器检查和启动"
    echo "  --no-auth               禁用身份验证"
    echo "  --token TOKEN           设置身份验证令牌"
    echo "  --max-connections N     最大连接数 (默认: 10)"
    echo "  -v, --verbose           详细日志输出"
    echo "  --release               使用 release 模式构建"
    echo ""
    echo "示例:"
    echo "  $0                      # 使用默认设置启动"
    echo "  $0 -p 8080 -v           # 在端口 8080 启动，详细日志"
    echo "  $0 -a my_pixel          # 启动指定的 AVD"
    echo "  $0 --skip-emulator      # 跳过模拟器检查"
    echo "  $0 --release            # 使用优化构建启动"
}

# 解析命令行参数
PORT=""
BIND=""
CONFIG=""
AVD_NAME=""
TIMEOUT=""
SKIP_EMULATOR=false
NO_AUTH=false
TOKEN=""
MAX_CONNECTIONS=""
VERBOSE=false
RELEASE_MODE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -p|--port)
            PORT="$2"
            shift 2
            ;;
        -b|--bind)
            BIND="$2"
            shift 2
            ;;
        -c|--config)
            CONFIG="$2"
            shift 2
            ;;
        -a|--avd)
            AVD_NAME="$2"
            shift 2
            ;;
        -t|--timeout)
            TIMEOUT="$2"
            shift 2
            ;;
        --skip-emulator)
            SKIP_EMULATOR=true
            shift
            ;;
        --no-auth)
            NO_AUTH=true
            shift
            ;;
        --token)
            TOKEN="$2"
            shift 2
            ;;
        --max-connections)
            MAX_CONNECTIONS="$2"
            shift 2
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        --release)
            RELEASE_MODE=true
            shift
            ;;
        -*)
            echo "未知选项: $1"
            show_help
            exit 1
            ;;
        *)
            echo "未知参数: $1"
            show_help
            exit 1
            ;;
    esac
done

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}🚀 Remote ADB Server 启动脚本${NC}"

# 检查 Rust 和 Cargo
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Cargo 未找到，请安装 Rust${NC}"
    exit 1
fi

echo -e "${GREEN}✅ Cargo 已找到${NC}"

# 构建项目
echo -e "${YELLOW}🔨 构建 Remote ADB Server...${NC}"

BUILD_FLAGS=""
if [ "$RELEASE_MODE" = true ]; then
    BUILD_FLAGS="--release"
    echo -e "${YELLOW}使用 release 模式构建${NC}"
fi

if ! cargo build $BUILD_FLAGS; then
    echo -e "${RED}❌ 构建失败${NC}"
    exit 1
fi

echo -e "${GREEN}✅ 构建成功${NC}"

# 准备启动参数
ARGS=()

if [ -n "$PORT" ]; then
    ARGS+=(--port "$PORT")
fi

if [ -n "$BIND" ]; then
    ARGS+=(--bind "$BIND")
fi

if [ -n "$CONFIG" ]; then
    ARGS+=(--config "$CONFIG")
fi

if [ -n "$AVD_NAME" ]; then
    ARGS+=(--avd-name "$AVD_NAME")
fi

if [ -n "$TIMEOUT" ]; then
    ARGS+=(--emulator-timeout "$TIMEOUT")
fi

if [ "$SKIP_EMULATOR" = true ]; then
    ARGS+=(--skip-emulator)
fi

if [ "$NO_AUTH" = true ]; then
    ARGS+=(--auth false)
fi

if [ -n "$TOKEN" ]; then
    ARGS+=(--token "$TOKEN")
fi

if [ -n "$MAX_CONNECTIONS" ]; then
    ARGS+=(--max-connections "$MAX_CONNECTIONS")
fi

if [ "$VERBOSE" = true ]; then
    ARGS+=(--verbose)
fi

# 确定可执行文件路径
if [ "$RELEASE_MODE" = true ]; then
    EXECUTABLE="./target/release/remote-adb-server"
else
    EXECUTABLE="./target/debug/remote-adb-server"
fi

# 显示启动信息
echo -e "${YELLOW}📋 启动配置:${NC}"
echo -e "  可执行文件: ${GREEN}$EXECUTABLE${NC}"
if [ -n "$PORT" ]; then
    echo -e "  端口: ${GREEN}$PORT${NC}"
fi
if [ -n "$BIND" ]; then
    echo -e "  绑定地址: ${GREEN}$BIND${NC}"
fi
if [ -n "$AVD_NAME" ]; then
    echo -e "  AVD 名称: ${GREEN}$AVD_NAME${NC}"
fi
if [ "$SKIP_EMULATOR" = true ]; then
    echo -e "  模拟器检查: ${YELLOW}跳过${NC}"
else
    echo -e "  模拟器检查: ${GREEN}启用${NC}"
fi
if [ "$VERBOSE" = true ]; then
    echo -e "  日志级别: ${GREEN}详细${NC}"
fi

echo ""

# 设置信号处理
cleanup() {
    echo -e "\n${YELLOW}🛑 收到停止信号，正在关闭服务器...${NC}"
    exit 0
}

trap cleanup SIGINT SIGTERM

# 启动服务器
echo -e "${GREEN}🚀 启动 Remote ADB Server...${NC}"
echo -e "${YELLOW}按 Ctrl+C 停止服务器${NC}"
echo ""

# 执行服务器
exec "$EXECUTABLE" "${ARGS[@]}"