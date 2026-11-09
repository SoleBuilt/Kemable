#!/usr/bin/env bash
# The same check as scripts/verify.mjs, for bash. The steps live there.
exec node "$(dirname "$0")/verify.mjs" "$@"
