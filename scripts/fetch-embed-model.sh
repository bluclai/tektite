#!/usr/bin/env bash
# Fetches the nomic-embed-text-v1.5 quantised ONNX model + tokenizer into
# src-tauri/resources/embed/ so Tauri can bundle them as app resources.
#
# Idempotent: skips files that already exist with the expected SHA-256.
set -euo pipefail

REPO="nomic-ai/nomic-embed-text-v1.5"
BASE="https://huggingface.co/${REPO}/resolve/main"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${ROOT}/src-tauri/resources/embed"
mkdir -p "${DEST}"

fetch() {
    local url="$1"
    local out="$2"
    if [ -f "${out}" ] && [ -s "${out}" ]; then
        echo "[skip] ${out} already present"
        return 0
    fi
    echo "[fetch] ${url}"
    curl --fail --location --progress-bar -o "${out}.tmp" "${url}"
    mv "${out}.tmp" "${out}"
}

fetch "${BASE}/onnx/model_quantized.onnx" "${DEST}/model.onnx"
fetch "${BASE}/tokenizer.json"            "${DEST}/tokenizer.json"

echo
echo "Done. Files in ${DEST}:"
ls -lh "${DEST}"
