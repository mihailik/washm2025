#!/usr/bin/env bash
set -euo pipefail

TAG=${1:-v3.14.0}
REPO=https://github.com/python/cpython.git
DEST=third_party/cpython

echo "Adding CPython submodule (tag=${TAG})"
if [ -d "$DEST" ]; then
  echo "CPython already present at $DEST"; exit 0;
fi
mkdir -p third_party
git submodule add --force --branch $TAG $REPO $DEST

echo "Initialized CPython at $DEST (pinned to $TAG). Run 'git submodule update --init --recursive' to fetch."