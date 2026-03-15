#!/bin/bash
# 手动体验 Gemini CLI 调用过程
# 复刻 missiond gemini_cli.rs 的完整调用方式
#
# Usage: ./scripts/test-gemini-cli.sh [--google|--apikey] [prompt_file]
#   --apikey  使用 API Key (默认，从 llm.yaml 读取)
#   --google  使用 Google One AI Pro (OAuth)
#   不带参数: 用内置测试 prompt

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MODEL="gemini-3.1-pro-preview"
IDLE_TIMEOUT=120
AUTH_MODE=""

# ── Parse args ──
PROMPT_FILE=""
for arg in "$@"; do
    case "$arg" in
        --google) AUTH_MODE="google" ;;
        --apikey) AUTH_MODE="apikey" ;;
        *) PROMPT_FILE="$arg" ;;
    esac
done

# Auto-detect from settings.json if not specified
if [[ -z "$AUTH_MODE" ]]; then
    AUTH_MODE=$("$SCRIPT_DIR/gemini-auth.sh" status 2>/dev/null | head -1 | awk '{print $NF}')
    [[ "$AUTH_MODE" != "apikey" && "$AUTH_MODE" != "google" ]] && AUTH_MODE="apikey"
fi

# ── Auth setup (switch settings.json + env var) ──
"$SCRIPT_DIR/gemini-auth.sh" "$AUTH_MODE" > /dev/null
if [[ "$AUTH_MODE" == "apikey" ]]; then
    if [[ -z "${GEMINI_API_KEY:-}" ]]; then
        LLM_YAML="$HOME/.xjp-mission/llm.yaml"
        if [[ -f "$LLM_YAML" ]]; then
            KEY=$(grep '^gemini_api_key:' "$LLM_YAML" | sed -E 's/^gemini_api_key: *"?([^"]*)"?/\1/')
            if [[ -n "$KEY" ]]; then
                export GEMINI_API_KEY="$KEY"
            fi
        fi
    fi
    echo "🔑 Auth: API Key (${GEMINI_API_KEY:0:10}...${GEMINI_API_KEY: -4})"
else
    unset GEMINI_API_KEY 2>/dev/null || true
    echo "🔑 Auth: Google One AI Pro (OAuth)"
fi

# ── Prompt ──
if [[ -n "$PROMPT_FILE" && -f "$PROMPT_FILE" ]]; then
    PROMPT=$(cat "$PROMPT_FILE")
    echo "📄 Prompt from file: $PROMPT_FILE (${#PROMPT} chars)"
else
    PROMPT="你好，请用一句话介绍自己。"
    echo "📄 Using built-in test prompt (${#PROMPT} chars)"
fi

echo "🤖 Model: $MODEL"
echo "⏱️  Idle timeout: ${IDLE_TIMEOUT}s"
echo ""
echo "━━━ Command ━━━"
echo "gemini -p <prompt> -m $MODEL -o stream-json --yolo"
echo ""
echo "━━━ Prompt Preview (first 200 chars) ━━━"
echo "${PROMPT:0:200}"
[[ ${#PROMPT} -gt 200 ]] && echo "... (${#PROMPT} chars total)"
echo ""
echo "━━━ Stream Output ━━━"

START_TIME=$(date +%s)
EVENT_COUNT=0
LAST_EVENT_TIME=$START_TIME

# 用 stream-json 模式调用，逐行读 NDJSON
# 这正是 gemini_cli.rs 的做法
gemini -p "$PROMPT" -m "$MODEL" -o stream-json --yolo 2>/tmp/gemini-cli-stderr.txt | while IFS= read -r line; do
    NOW=$(date +%s)
    ELAPSED=$((NOW - START_TIME))
    DELTA=$((NOW - LAST_EVENT_TIME))
    LAST_EVENT_TIME=$NOW
    EVENT_COUNT=$((EVENT_COUNT + 1))

    # 跳过空行
    [[ -z "${line// }" ]] && continue

    # 解析 event type
    TYPE=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('type','?'))" 2>/dev/null || echo "parse-error")
    ROLE=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('role',''))" 2>/dev/null || echo "")
    CONTENT_LEN=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); c=d.get('content',''); print(len(c) if c else 0)" 2>/dev/null || echo "0")

    # 格式化输出
    printf "[%3ds +%2ds] #%-3d %-10s" "$ELAPSED" "$DELTA" "$EVENT_COUNT" "$TYPE"
    if [[ -n "$ROLE" ]]; then
        printf " role=%-9s" "$ROLE"
    else
        printf "               "
    fi
    if [[ "$CONTENT_LEN" -gt 0 ]]; then
        printf " content=%s chars" "$CONTENT_LEN"
    fi
    echo ""

    # result/error 是终止事件
    if [[ "$TYPE" == "result" || "$TYPE" == "error" ]]; then
        echo ""
        echo "━━━ Terminal Event ━━━"
        echo "$line" | python3 -m json.tool 2>/dev/null || echo "$line"
    fi
done

END_TIME=$(date +%s)
TOTAL=$((END_TIME - START_TIME))

echo ""
echo "━━━ Summary ━━━"
echo "Total: ${TOTAL}s | Auth: $AUTH_MODE"

# 显示 stderr（如果有）
if [[ -s /tmp/gemini-cli-stderr.txt ]]; then
    echo ""
    echo "━━━ Stderr ━━━"
    head -20 /tmp/gemini-cli-stderr.txt
fi
