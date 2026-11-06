<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Compatibility

What a Norn build promises about the data it finds on disk and the clients it
talks to, from 1.0 on. The rule from SPEC §40: never silently break stored data, and
an old client or an old build fails clearly rather than behaving incorrectly.

## The versions on disk

Every durable file carries its own magic and version, checked before anything else
in it is believed. `<database>/FORMAT` carries the version of the store as a whole.

| What | Where | Version today | A build meeting a newer one |
| --- | --- | --- | --- |
| Store | `FORMAT` (16 bytes; magic `NORNDB\0\0`, version, CRC-32C) | 1 | refuses to start, changes nothing |
| Segment container | segment header, bytes 8 and 9: major, minor | 1.0; 1.1 only when encrypted | that stream refuses; the daemon does not start; the file is untouched |
| Chunk schema | segment header, `schema_version` | 1 | as above |
| Chunk frame | frame header, byte 4 | 1 | as above |
| Block codec | segment header, `codec_version` | see `norn_codec::CODEC_VERSION` | as above |
| Journal | ordinary segments under `<stream>/journal/` | as a segment | as a segment |
| Metadata log | `metadata.log` header (`NORNMETA`, u32) | 2 (reads 1, upgrades it in place on open) | refuses to start, changes nothing |
| Clean marker | `<stream>/CLEAN` | 1 | ignored: the stream's chunks are read and checked instead |
| Index snapshot | `index/<stream>/summary.nidx`, `signals.nsig`, `*.ndet` | 1 each | discarded and rebuilt from the segments |
| Backup | `MANIFEST`, first line | 1 | `restore` refuses, changes nothing |

Two kinds of file, then. **Source of truth**: segments, the journal and the metadata
log, which are never discarded, and a version this build does not know stops it
serving. **Derived**: the clean marker and the index, which can be rebuilt, so a
version this build does not know costs only time at startup.

## What each build reads and writes

A daemon reads every version it has ever written within its support window, and writes
only the current one. The exceptions are stated, not implied:

- A **plain segment is written as 1.0** for as long as that layout is enough. A reader
  checks a segment's version file by file, so a store may mix 1.0 and 1.1 segments.
- A **metadata log** of version 1 is upgraded where it lies, by rewriting its header on
  open. It is the one file a build rewrites without being asked; version 1 is a strict
  subset of version 2, so nothing else about it changes.
- **A build never rewrites a segment** to a newer version just because it can read it.
  That is what `nornctl migrate` is for, and only when a new store version calls for it.

### When the store version moves

The store version moves only when some file must be rewritten before a build can serve
the database. Everything else is handled at the file's own version:

| Change | Segment version | Store version |
| --- | --- | --- |
| A header flag or record readers can tell apart, such as `encrypted` | minor +1, only for files that use it | unchanged |
| A new column kind, codec or compression a reader can tell apart | schema or codec +1 | unchanged |
| A layout old files cannot be read through | major +1 | +1, with a migration |
| A new metadata record kind | log version +1, read in place | unchanged |
| A metadata layout old logs cannot be read through | log version +1 | +1, with a migration |

So encryption (segment format 1.1) leaves a store at version 1. An unencrypted store
written after encryption landed is still, byte for byte, the format this page's golden
stores pin. A build from before it refuses a 1.1 segment as `UnsupportedVersion`, naming
the file, and modifies nothing, which is the failure this policy wants.

### A daemon meeting a store

Checked at startup, before the registry or any segment is opened:

| Found | What the daemon does |
| --- | --- |
| No `FORMAT` file | store version 1, as written by 1.0.0-rc.1 and earlier: read in place, and the file is added |
| `RESTORING` | refuses: run `nornctl restore` again |
| `MIGRATION` | refuses: run `nornctl migrate` again |
| Newer than this build | refuses: `store version N was written by a newer Norn`, nothing changed |
| Older, and migratable | refuses: `run nornctl migrate --data-dir D --to V` |
| Older than the window | refuses: go through a release that still migrates it |

Today's window holds one version, so the last two cannot yet happen outside a test.

## Support window

- A release reads every store version of its own major and of the last release of the
  previous major, in place or through `nornctl migrate`.
- A store version a release reads in place stays read in place for the rest of that
  major: `CURRENT_VERSION`, `OLDEST_READ` and `OLDEST_MIGRATED` in `norn-database` state it.
- **Downgrades are not supported** once a newer build has written a newer version, and
  every version check says so rather than guessing. Within one store version a downgrade
  works, unless encrypted (1.1) segments were written. A migration with `--keep-old`
  keeps the older files until you remove them, and that is the way back.
- A backup restores on any build whose window includes its store version.

## The protocol

A protocol version is `major.minor` ([protocol](protocol.md) has the details and the
history). This build speaks `norn_protocol::PROTOCOL_VERSION`.

- **Major** is the `vN` of the `norn.*` packages, and changes only with a new package.
  A client of another major is refused with `ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED`.
- **Minor** counts compatible additions within a major: methods, fields, enum values,
  reasons, capabilities. `buf breaking` enforces that a minor never breaks the wire or
  JSON form. Client and daemon agree the lower of the two minors, and a daemon answers
  a method newer than it `UNIMPLEMENTED`.
- A daemon serves every minor of its major for as long as that major exists.

The store and the protocol version independently: a new store version does not imply a
new protocol, nor the reverse.

## What holds this in place

- `crates/runtime/tests/golden/v1/` holds stores written by this version, checked in:
  `clean` (sealed segments, a compaction output, clean markers, index snapshots),
  `crash` (acknowledged batches only in the journal) and `metadata-v1` (the older log
  header). They are unencrypted, and stay valid after encryption landed: a plain segment
  is still format 1.0.
- `tests/golden_read.rs` opens private copies with the current code on every CI run and
  requires each sample back, no damage reported, the markers and index snapshots accepted,
  and the version bytes this page states. `tests/format.rs` in `norn-storage` pins the
  offsets.
- **If `golden_read` fails, the change broke every database in the field.** Do not
  regenerate the golden stores to make it pass. Keep reading the old form, and bump the
  relevant version as the table above says. When a new version is added, add a new
  directory beside `v1` (written by `tests/golden_write.rs`, on purpose, with
  `NORN_WRITE_GOLDEN=1`) and keep every older one for as long as the window covers it.
