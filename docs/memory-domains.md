# Memory domains

A worker's *domain* is the memory in which its state lives, and in which its agents and its leader
address it at the same addresses (`CONTRACT.md`, Terms). `trame::Alloc` puts everything a state or
the pool owns there.

## Why

The leader builds a state, and the worker's agents use it in place. On some backends the agents run
in another process or on a device. A `Vec` that the state owns would then hold a pointer into the
leader's heap, which means nothing there. The domain makes the state's bytes, with every byte it
owns, lie where the agents can address them (`CONTRACT.md`, M1).

## Alloc

`trame::Alloc` is a global allocator that wraps the allocator a program would use otherwise:

```rust
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
```

Every trame program installs it. `Topology::launch` refuses a program without it
(`Refused::Alloc`), on every backend, so the source of a program is the same whichever backend its
features select.

## Arena mode

*Arena mode* is a flag of the calling thread. The runtime sets it while a builder (`Topology::set`)
or an edit (`Run::edit`) runs. An allocation made in arena mode asks the backend for domain memory;
outside arena mode, every allocation goes to the wrapped allocator.

```mermaid
%%{init: {'theme':'base','themeVariables':{'primaryColor':'#ffffff','primaryTextColor':'#000000','primaryBorderColor':'#000000','lineColor':'#000000','secondaryColor':'#ffffff','tertiaryColor':'#ffffff','clusterBkg':'#ffffff','clusterBorder':'#000000','edgeLabelBackground':'#ffffff','actorBkg':'#ffffff','actorBorder':'#000000','actorTextColor':'#000000','actorLineColor':'#000000','signalColor':'#000000','signalTextColor':'#000000','noteBkgColor':'#ffffff','noteBorderColor':'#000000','noteTextColor':'#000000','labelBoxBkgColor':'#ffffff','labelBoxBorderColor':'#000000','fontFamily':'monospace'},'themeCSS':'rect.actor,rect.note,.note rect,.statediagram-note rect{fill:#fff!important;stroke:#000!important} line,.actor-line{stroke:#000!important} .arrowheadPath,marker path{fill:#000!important;stroke:#000!important} circle.state-end{stroke:#000!important} text,.noteText,.state-note text{fill:#000!important}'}}%%
flowchart TD
  A["alloc"] --> M{"arena mode<br/>on this thread?"}
  M -- no --> H["wrapped allocator"]
  M -- yes --> D{"backend answers<br/>domain memory?"}
  D -- yes --> DM["domain block"]
  D -- "no: heap is the domain" --> H
  F["free"] --> C{"address in a domain?"}
  C -- yes --> DF["domain free"]
  C -- no --> HF["wrapped free"]
```

A block that lies in a domain is freed and resized in it, whoever does it. A state taken by the
leader can therefore be dropped or grown by the leader. A heap block that grows in arena mode moves
into the domain. The cost outside arena mode is one thread-local read, and on free one range check.

## Domains by backend

**seq.** The domain is the process heap. The wrapper forwards every call.

**mpi.** The domain is a shared segment of the node, mapped at one address in every rank of it
(`backends/mpi/segment.rs`).

## Bound

A domain has a size fixed at launch. An allocation that does not fit allocates nothing outside the
domain: the backend says the domain is full, and the process stops (`CONTRACT.md`, M6).

## What a state holds

A state holds no value whose meaning depends on the process that made it: no reference, raw
pointer, function pointer, trait object, `Rc` or handle. This is a type, `trame::Plain`, which
every field must have; a user type gets it from `#[derive(trame::Plain)]`, which requires it of
every field in turn. Steps neither allocate nor free (`CONTRACT.md`, M2 to M4). The driver refuses allocation in items, and `tests/alloc.rs` counts
allocations in steps (`PROOFS.md`, M4).
