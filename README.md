# hartig

An [OHTTP](https://www.rfc-editor.org/rfc/rfc9458.html) relay and
gateway/target-resource service that facilitates the **fungi** multiparty
Bitcoin privacy protocol for **constrained clients** — web wallets (limited to
HTTP / WebSockets / WebRTC, none of which provide transport-layer metadata
privacy; OHTTP is usable from bare `fetch`) and mobile wallets (for which Tor
is impractical in the background: circuit bootstrap is ~23–27 RTTs, OHTTP
needs ~3–5 per request).

## What it does

- **Relay** — forwards encapsulated OHTTP requests to peer gateways (sees
  ciphertext + client metadata, never plaintext).
- **Gateway / target resources** — terminates OHTTP and serves:
  - **linked mailboxes** — ephemeral append-only P2P rendezvous slots.
  - **broadcast channels** — per-topic grow-only replayable message sets.
  - **key-consistency service** — multi-path (and optionally attested)
    verification that a gateway is not equivocating on its keys.

Clients route through at least one *other* operator's relay to reach a
gateway, so no single operator sees both who is asking and what is asked.
When a TEE is available, all key material (HPKE and TLS) is generated inside
the enclave and never leaves it — reducing how much a client must trust the
operator; attestation is optional hardening, not a protocol requirement.

## Not a dependency

fungi does not depend on hartig. Full peers use peer-to-peer gossip; hartig
is a facultative fast path and bridge for clients that would otherwise
struggle to participate, and resilience comes from a mesh of independently
operated instances, never any single one. (Hence the name: the *Hartig net*
is the hyphal interface of ectomycorrhizal symbiosis — likewise facultative
and external.)

## Where the design lives

Design and planning are recorded in the commit graph, not in prose files:
tickets are empty `WIP:` commits whose parent edges are dependencies and
whose messages are the design record. [AGENTS.md](./AGENTS.md) explains how
to read and work on the graph.
