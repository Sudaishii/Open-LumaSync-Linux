#!/usr/bin/env bash
set -euo pipefail
integration_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec "$integration_dir/../../install.sh" --omarchy "$@"
