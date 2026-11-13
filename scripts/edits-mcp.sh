#!/bin/sh
# Starts the edits MCP server for an agent (used by .mcp.json).
# It runs the already-built binary, so the client does not wait for a compile.
# Build once first:
#   cargo build --release -p edits-cli -p edits-analyze -p edits-mcp
here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bin="$here/target/release/edits-mcp"
if [ ! -x "$bin" ] || [ ! -x "$here/target/release/edits" ]; then
    echo "edits MCP server is not built yet. In $here run:" >&2
    echo "  cargo build --release -p edits-cli -p edits-analyze -p edits-mcp" >&2
    exit 1
fi
exec "$bin" "$@"
