#!/bin/bash
# 手动体验 Gemini CLI 调用过程
# 复刻 missiond gemini_cli.rs 的完整调用方式
#
# Usage: ./scripts/test-gemini-cli.sh [prompt_file]
#   不带参数: 用内置测试 prompt
#   带文件路径: 从文件读取 prompt

set -euo pipefail

MODEL="gemini-3.1-pro-preview"
IDLE_TIMEOUT=120

# ── Prompt ──
if [[ $# -ge 1 && -f "$1" ]]; then
    PROMPT=$(cat "$1")
    echo "📄 Prompt from file: $1 (${#PROMPT} chars)"
else
    PROMPT="你好，请用一句话介绍自己。"
    echo "📄 Using built-in test prompt (${#PROMPT} chars)"
fi

echo "🤖 Model: $MODEL"
echo "⏱️  Idle timeout: ${IDLE_TIMEOUT}s"
echo ""
echo "━━━ Command ━━━"
echo "gemini -p <prompt> -m $MODEL -o stream-json"
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
gemini -p "$PROMPT" -m "$MODEL" -o stream-json 2>/tmp/gemini-cli-stderr.txt | while IFS= read -r line; do
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
echo "Total: ${TOTAL}s"

# 显示 stderr（如果有）
if [[ -s /tmp/gemini-cli-stderr.txt ]]; then
    echo ""
    echo "━━━ Stderr ━━━"
    head -20 /tmp/gemini-cli-stderr.txt
fi
