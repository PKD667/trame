# The pool

The pool is read-only data shared by every worker of a topology. Steps read it, and only the leader
writes it, in an edit (`CONTRACT.md`, P1, P2).

## Declare and fill

A state names the pool type its steps read: `#[trame::state(steps(..), pool = P)]`. A step reads the
pool with a parameter `pool: &P`. `#[trame::step]` refuses a `&mut` pool.

The topology declares the pool and gives it a value as it does a worker:

```rust
let p = t.pool::<Table>();
t.set(&p, || Table { weights: vec![0.5; 1024] });
```

A topology has one pool. Launch refuses a worker whose steps read a pool of another type
(`Refused::PoolType`) and a declared pool that was never set (`Refused::PoolUnset`). A worker whose steps read a pool, in a topology
that declares none, is refused the same way.

## Pool bit

The pool is one more field in every worker's locks: bit 63 of the read sets (`POOL` in `sched.rs`).
A step that takes `pool: &P` reads that bit. A step that does not never conflicts with the pool.

## Edit

`run.edit(&p, |v| ..)` is an edit that holds the pool bit in every worker at once. Only steps that
read the pool wait. The closure runs once no step that reads the pool runs, in any worker. Then
every worker gets the bit back, and the steps that read it are woken. A step that starts after the
edit returns reads the edited pool, and none reads it during the edit (`CONTRACT.md`, M5).

```mermaid
%%{init: {'theme':'base','themeVariables':{'primaryColor':'#ffffff','primaryTextColor':'#000000','primaryBorderColor':'#000000','lineColor':'#000000','secondaryColor':'#ffffff','tertiaryColor':'#ffffff','clusterBkg':'#ffffff','clusterBorder':'#000000','edgeLabelBackground':'#ffffff','actorBkg':'#ffffff','actorBorder':'#000000','actorTextColor':'#000000','actorLineColor':'#000000','signalColor':'#000000','signalTextColor':'#000000','noteBkgColor':'#ffffff','noteBorderColor':'#000000','noteTextColor':'#000000','labelBoxBkgColor':'#ffffff','labelBoxBorderColor':'#000000','fontFamily':'monospace'},'themeCSS':'rect.actor,rect.note,.note rect,.statediagram-note rect{fill:#fff!important;stroke:#000!important} line,.actor-line{stroke:#000!important} .arrowheadPath,marker path{fill:#000!important;stroke:#000!important} circle.state-end{stroke:#000!important} text,.noteText,.state-note text{fill:#000!important}'}}%%
sequenceDiagram
  participant L as leader
  participant W1 as worker 1
  participant W2 as worker 2
  L->>W1: Hold POOL
  L->>W2: Hold POOL
  W1-->>L: no step reads the pool
  W2-->>L: last reader ends
  Note over L: f runs on the pool, may allocate
  L->>W1: Edit POOL, readers woken
  L->>W2: Edit POOL, readers woken
```

The leader takes the holds one worker after another. This cannot deadlock: only the leader holds more
than one, and a step never waits for a hold.

## Asking for a change

A worker that wants the pool changed sends the leader a message. The step that sends returns, which
gives its read of the pool back, and the leader edits. A worker cannot write the pool, so no step
needs to wait for another worker to finish reading.
