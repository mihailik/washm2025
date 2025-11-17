#!/usr/bin/env bash
set -euo pipefail

TAG=${1:-v2025-10-01}
REPO=https://github.com/bellard/quickjs.git
DEST=third_party/quickjs

echo "Adding QuickJS submodule (tag=${TAG})"
if [ -d "$DEST" ]; then
  echo "QuickJS already present at $DEST"; exit 0;
fi
mkdir -p third_party
git submodule add --force --branch $TAG $REPO $DEST

echo "Initialized QuickJS at $DEST (pinned to $TAG). Run 'git submodule update --init --recursive' to fetch."