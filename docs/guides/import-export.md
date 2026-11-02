# Import and export

Moving history between Norn and other tools: CSV for anything, Parquet and Arrow for
analysis, MCAP for Foxglove and Lichtblick. Everything goes through the public API,
so it works against a running daemon and needs nothing else. Output below is real,
from `nornctl` 0.1.0 on macOS. Exact formats are in [adapters](../reference/adapters.md).

## Export

```console
$ nornctl export all --format csv --output plant.csv
exported samples=3 signals=2 bytes=683 to plant.csv
$ cat plant.csv
# norn.csv 1 {"version":1,"streams":[{"name":"streams/plant"}],"entities":[{"name":"entities/ahu-01"}],"signals":[{"name":"entities/ahu-01/signals/fan-on","value_type":"bool","stream":"streams/plant"},{"name":"entities/ahu-01/signals/supply-temp","value_type":"double","stream":"streams/plant"}]}
time,signal,stream,type,value,severity,substatus,sequence
2026-11-02T08:00:00.000000000Z,entities/ahu-01/signals/fan-on,streams/plant,bool,true,GOOD,,0
2026-11-02T08:00:00.000000000Z,entities/ahu-01/signals/supply-temp,streams/plant,double,21.5,GOOD,,0
2026-11-02T08:00:01.000000000Z,entities/ahu-01/signals/supply-temp,streams/plant,double,NaN,GOOD,,1
# norn.end rows=3 crc32=9100b497
$ nornctl export all --format parquet --output plant.parquet
exported samples=3 signals=2 bytes=4408 to plant.parquet
$ nornctl export all --format mcap --output plant.mcap
exported samples=3 signals=2 bytes=39087 to plant.mcap
$ nornctl export all --format arrow --output plant.arrow
exported samples=3 signals=2 bytes=4826 to plant.arrow
```

`TARGET` selects as `query` does: a signal's full name, an entity, or `all`. Add
`--from` and `--to` for an interval, or `--select` for a signal filter. An export
never overwrites a file:

```console
$ nornctl export all --format csv --output plant.csv
nornctl: creating the output, which must not exist: File exists (os error 17)
```

If an export fails partway, its file is removed. Samples acknowledged while an
export runs may or may not be in it.

## Import

Into a fresh daemon, from any of the four files:

```console
$ nornctl import plant.parquet --ack persisted
imported read=3 sent=3 present=0 duplicate=0 created=4 batches=1 throttled=0 retried=0
$ nornctl signals
entities/ahu-01/signals/fan-on VALUE_TYPE_BOOL streams/plant
entities/ahu-01/signals/supply-temp VALUE_TYPE_DOUBLE streams/plant
$ nornctl query all
2026-11-02T08:00:00Z entities/ahu-01/signals/fan-on true GOOD seq=0
2026-11-02T08:00:00Z entities/ahu-01/signals/supply-temp 21.5 GOOD seq=0
2026-11-02T08:00:01Z entities/ahu-01/signals/supply-temp NaN GOOD seq=1
```

`created=4` is the stream, the entity and both signals, with their display names,
units and annotations from the file. The format comes from the extension, or from
`--format`. `--ack` defaults to `durable`, and `--batch N` caps samples per batch.

Importing is safe to repeat. The same samples from another format add nothing:

```console
$ nornctl import plant.mcap --ack persisted
imported read=3 sent=0 present=3 duplicate=0 created=0 batches=0 throttled=0 retried=0
```

`present` counts samples the daemon already held, which were never sent.
`duplicate` counts those the daemon itself recognised as resends.

## Damage

A damaged file stops the import at the row, with everything before it stored:

```console
$ nornctl import cut.csv
nornctl: csv row 3: truncated: the row is cut off
```

Parquet, Arrow and MCAP report a failed checksum as `corrupt`, with the first row of
the damaged row group, batch or chunk where it can be named.

## Files from other tools

- **CSV by hand**: the header line alone is enough. Signals are created from the rows,
  without metadata:

  ```text
  time,signal,stream,type,value,severity,substatus,sequence
  2026-11-02T08:00:00Z,entities/ahu-01/signals/supply-temp,streams/plant,double,21.5,GOOD,,0
  ```

- **pandas**: `pd.read_csv("plant.csv", skiprows=1, skipfooter=1, engine="python",
  keep_default_na=False)`, or `pd.read_parquet("plant.parquet")`.
- **MCAP**: open it in Foxglove or Lichtblick. Each signal is a topic, and messages
  decode from the schema in the file. `mcap info plant.mcap` lists them:

  ```text
  channels:
  	(1) entities/ahu-01/signals/fan-on     	1 msgs         	 : norn.type.Sample [protobuf]
  	(2) entities/ahu-01/signals/supply-temp	2 msgs (1..2Hz)	 : norn.type.Sample [protobuf]
  ```

Compressed MCAP and zstd Parquet from other tools are refused, because their codecs
would be C ([adapters](../reference/adapters.md#formats)).
