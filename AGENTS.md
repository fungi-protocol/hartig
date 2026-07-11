# Agent conventions for this repository

## Use jj

This repo is operated with [jj](https://jj-vcs.github.io/) (colocated with
git). Use `jj` for all history operations; do not use `git` write commands.
Refer to commits by **change ID** (stable across rewrites), not commit ID.
Before any graph surgery, note the current operation (`jj op log -n1`) as a
restore point; `jj op restore` undoes mistakes.

## The change graph is a DAG — work on it as one

History in this repository is not a linear branch with tickets bolted on:
the commit graph itself is the data structure. Parent edges carry meaning —
dependency and derivation — so structure your work as a DAG and preserve
the edges' semantics through every operation.

Planning commits ("tickets" — the role GitHub issues would play elsewhere)
are just the special case of commits that **start empty**: a `WIP:` commit
whose parent edges are its dependencies and whose description is its design
record. Implementation fills them in; they are not a separate tracking
system layered over the graph, they are nodes of it.

Reading the graph: `jj log` / `git log --graph` is the ticket board;
`jj show <rev>` reads a ticket's design record. Bookmarks: `tickets` marks
the roadmap head (a merge of all leaf tickets, so every ticket is reachable
from one ref), `v1` marks the MVP milestone (its ancestry is exactly the v1
scope), and `main` is the trunk.

- **One place per fact.** Planning and design information lives only in the
  relevant commit's description. Never mirror it into README.md or other
  files; update the description with `jj describe -r <change-id>`.
- **Respect dependencies.** Do not start a ticket whose parent tickets are
  unimplemented. A commit's ancestry is its full set of prerequisites; the
  `v1` bookmark's ancestry is the MVP scope. Independent work belongs on
  independent branches of the graph (an antichain), not serialized
  arbitrarily.
- **Implement in place.** Work on a ticket happens at its node in the
  graph: build the implementation as real commits occupying the ticket's
  position (same parents), keeping the design record in the final commit
  message and dropping the `WIP:` prefix only when the ticket is complete.
  Descendant tickets rebase onto the result.
- **Preserve reachability.** Every ticket must remain reachable from the
  `tickets` bookmark (the roadmap-head merge of all leaves). If you add or
  complete leaf tickets, rebuild the roadmap-head merge accordingly.
- **No structure-destroying rewrites.** Do not squash tickets together,
  abandon tickets, or reparent in ways that change dependency semantics
  without explicit human authorization. When rebasing, prefer
  `jj rebase -s` (moves the subtree); never `jj rebase -r` on a non-leaf
  commit. Check for divergent change IDs before and after graph edits.
- **Splitting is fine, losing is not.** A ticket may be split into finer
  tickets (new empty `WIP:` commits inheriting the relevant edges and the
  relevant slice of the description), and new planning commits may be added
  anywhere their dependencies exist — but information from an original
  description must survive somewhere in the DAG.

## Design provenance

The design began as a consolidated pre-implementation specification
(SPEC.md, including the outcome of an adversarial design review) that was
then distributed into the ticket descriptions; prior art is cited on the
tickets it informed. The original document remains recoverable from the
root commit's evolution log (`jj evolog -r 'root()+'`) if ever needed. The
fungi protocol context is in `~/code/multiparty-protocol-docs` (overview
branch).

## Nix

The build is nix-based and **hermetic**:

- Use `nix develop` for the development environment. Do not install
  toolchains or dependencies through system package managers; if a tool is
  missing, add it to the devshell.
- The primary deliverable is a reproducible **nix2container-based Docker
  image** (see the scaffolding ticket). Reproducibility is load-bearing:
  the image measurement feeds the TEE attestation story, so do not
  introduce impure or non-deterministic build steps.
- Verify work with the flake: `nix build` / `nix flake check` must pass for
  a commit to count as complete, once the scaffolding ticket lands them.
