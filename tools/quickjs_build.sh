#!/usr/bin/env bash
set -euo pipefail

# convenience wrapper to build QuickJS: calls into third_party/quickjs using emsdk
QUICKJS_DIR=${1:-third_party/quickjs}
if [ ! -d "$QUICKJS_DIR" ]; then
  echo "QuickJS not found at $QUICKJS_DIR. Run tools/add_quickjs.sh first.";
  exit 1
fi
if ! command -v emcc >/dev/null; then
  echo "Emscripten (emcc) not found. Install emsdk and run ./emsdk activate latest then source ./emsdk_env.sh";
  exit 1
fi

pushd $QUICKJS_DIR
emcc -O3 -s WASM=1 -s ASYNCIFY=1 -s ALLOW_TABLE_GROWTH=1 -I. src/quickjs.c src/cutils.c src/libregexp.c src/quickjs-lib.c -o quickjs.wasm
mv quickjs.wasm ../../host/quickjs-wasm.wasm
popd

echo "QuickJS wasm built: host/quickjs-wasm.wasm"