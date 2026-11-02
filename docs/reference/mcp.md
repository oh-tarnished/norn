# MCP

> **Not implemented yet.** No code exists for this. The MCP and CLI piece fills this
> document in before writing code. Below are the requirements from
> [spec.md](../project/spec.md), as the design target.

## Requirements

- An optional, independent MCP server executable above Norn: AI agent → MCP
  server → Norn gRPC client → daemon. It never touches storage (§31).
- Tools: list_entities, list_signals, get_signal_metadata, query_range,
  get_latest, state_at, find_events, replay, compare_periods,
  get_statistics (§31).
- No AI reasoning inside Norn; it supplies deterministic temporal
  primitives (§32).
