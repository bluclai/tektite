# Embedding model resources

Run `scripts/fetch-embed-model.sh` from the repo root to download the
nomic-embed-text-v1.5 quantised ONNX model and tokenizer into this
directory. Files produced:

- `model.onnx` — quantised ONNX graph (~33 MB)
- `tokenizer.json` — HuggingFace tokenizer config

Tauri bundles everything in this directory as an app resource. Until the
model is downloaded the app falls back to `FakeEmbedder` at runtime and
semantic search returns stable-but-not-meaningful results.
