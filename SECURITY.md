# Security

## Reporting

Report a vulnerability privately to the repository owner through GitHub's
private vulnerability reporting, not a public issue. Expect an
acknowledgement before any fix is discussed in public.

## What exists today

- **Local IPC only.** A Unix socket inside an owner-only (0700) directory, or a
  named pipe whose code has not yet been run on Windows. The daemon refuses a more
  open directory. Loopback TCP exists for tests and has no OS identity check.
  [docs/reference/ipc.md](docs/reference/ipc.md#security-model-spec-41) has the model.
- **Bounded resources.** Connections, in-flight batches, batch size, metadata and
  resource counts have explicit limits, and decode memory is charged to a budget, so
  a client gets a refusal rather than exhausting the daemon.
- **Checksummed storage.** Damage is detected and reported, never silently
  discarded ([docs/reference/storage.md](docs/reference/storage.md#recovery)).
- **Decoders never panic** on malformed input, and allocate only what their input
  implies. Value decoding is property-tested on arbitrary input.
- **Supply chain.** `cargo-deny` checks advisories, licences and sources in CI, and
  `xtask deps` keeps C out of the graph.

## Not done yet

No security review has been carried out, and there is no fuzzing (stage 16). Limits
on queries, replay sessions and subscriptions wait for those subsystems. Shared
memory does not exist. Data at rest is not encrypted.
