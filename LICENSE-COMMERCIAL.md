# Commercial licence

Norn is copyright © 2026 Srikanth K, who holds all rights in it and is the
only party who can grant a commercial licence. It is dual-licensed.

| You are | Your licence | Cost |
|---|---|---|
| Building open-source software whose licence is compatible with AGPL-3.0 | [AGPL-3.0-or-later](./LICENSE) | Free, forever |
| Building proprietary or closed-source software on Norn's code | Commercial licence | Paid |
| Embedding Norn in a product you distribute without source | Commercial licence | Paid |
| Offering Norn, or a service built on it, over a network without publishing your source | Commercial licence | Paid |

Every part of Norn is under these terms, with no exception: the engine, the
server, the client crates (`norn-ipc`, `norn-transport`), the protocol crate
`norn-protocol` with its generated code, and the `.proto` definitions under
[`protobuf/`](./protobuf). Building a proprietary client with any of them,
including generating code from the `.proto` files, needs a commercial licence.

---

## Why it works this way

The AGPL does the sorting, so no one has to argue about what counts as "open
source" or "proprietary". If your project's licence is AGPL-compatible you
already comply and owe nothing. If your product is closed, the AGPL is
incompatible with how you ship, and a commercial licence is the way through.

The intent is straightforward: **free for people building in the open, paid for
people building on top of it commercially without giving anything back.**

## What the AGPL asks of you

Section 13 is the part most people have not read. If you modify Norn and let
users interact with it over a network, you must offer those users the source
of your modified version, even if you
never distribute a binary. That closes the gap the plain GPL leaves open for
hosted services, and it is deliberate.

Running the unmodified server, for your own data or your customers', and
calling it from your own proprietary applications, is ordinary use: the AGPL
asks nothing of the applications that only talk to it.

Note that **linking counts.** A Rust program that links any Norn crate,
`norn-protocol`, `norn-ipc` and `norn-transport` included, is a derived work,
and if that program is proprietary it needs a commercial licence.

## What a commercial licence grants

- The right to modify, embed and distribute Norn in closed-source products
- No obligation to publish your source under Section 13
- Terms negotiated per deployment: sites, instances, redistribution, support

## Getting one

Open an issue titled `commercial licence`, or contact Srikanth K, the copyright
holder, through the repository. Include roughly what you are building and how you intend to deploy
it — that is usually enough to size it.

---

**This file describes the offer. It is not itself a contract**, and it does not
grant rights. An executed commercial agreement with Srikanth K does. Nothing here is legal
advice; if the licensing boundary matters to your business, have a lawyer read
both this and the AGPL before you rely on either.
