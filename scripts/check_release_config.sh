#!/usr/bin/env bash
# 检查发布时的配置版本
# 规则：新版本配置版本号 = 上一版本配置版本号 + 1
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

VERSION_FILE="assets/shell-integration/config_version.txt"
HIGHLIGHTS_FILE="assets/shell-integration/config_update_highlights.tsv"

echo "=== 配置版本检查 ==="
echo ""

# 获取当前配置版本号
current_config_version=$(cat "$VERSION_FILE" | tr -d '[:space:]')
echo "当前配置版本号: $current_config_version"

# 计算期望的配置版本号
expected_config_version=$((current_config_version + 1))
echo "新版本配置版本号应为: $expected_config_version"
echo ""

# 检查 highlights 文件是否包含新版本的更新内容
new_highlights=$(grep "^$expected_config_version	" "$HIGHLIGHTS_FILE" 2>/dev/null || echo "")

if [[ -z "$new_highlights" ]]; then
    echo "⚠️  未找到版本 $expected_config_version 的更新内容"
    echo ""
    echo "如果本次发布需要更新配置，请在 $HIGHLIGHTS_FILE 中添加："
    echo "$expected_config_version	<更新内容（英文）>"
    echo "$expected_config_version	<更新内容（中文）>"
    echo ""
    echo "当前 highlights 文件中的版本:"
    cut -f1 "$HIGHLIGHTS_FILE" | sort -u -n | tail -5
    exit 1
else
    echo "✓ 找到版本 $expected_config_version 的更新内容:"
    echo "$new_highlights" | head -3
    echo ""

    # 统计该版本的条目数
    count=$(echo "$new_highlights" | wc -l)
    echo "共 $count 条更新说明"

    if [[ $count -lt 2 ]]; then
        echo "⚠️  建议至少提供 2 条更新说明（中英文各一条）"
    fi
fi

echo ""
echo "✓ 配置版本检查通过"
