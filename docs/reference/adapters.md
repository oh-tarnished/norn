# Adapters

> **Not implemented yet.** No code exists for this. The adapters piece fills this
> document in before writing code. Below are the requirements from
> [spec.md](../project/spec.md), as the design target.

## Requirements

- Adapters live outside the core, behind one adapter contract, and convert
  external data into Entity, Signal, Sample and Metadata (§29).
- External system → adapter → Norn protocol → Norn; never the internal
  storage API (§47).
- Planned: BACnet (through `bacnet-rs`), Modbus, OPC-UA, MQTT, MCAP, Parquet,
  CSV, custom Protobuf (§29, §38).
- Domain knowledge stays in metadata. A BACnet object becomes an entity, a
  signal and metadata such as `protocol=BACnet`, `device_instance=1001`,
  `object_type=analogInput`, `unit=degree_celsius` (§11, §30). This is
  already possible with `norn_core::Metadata`.
- Protocol status flags map onto `norn_core::Quality` (severity plus a
  generic substatus); the original flags may be kept in metadata.
