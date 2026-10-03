#!/usr/bin/env bash
# Usage: register.sh <name> <password> [--admin]
set -euo pipefail
source "$(dirname "$0")/lib.sh"
register_user "$@"
