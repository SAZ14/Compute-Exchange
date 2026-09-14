# Compute Exchange

A Rust marketplace for compute capacity, built in stages toward real GPU job execution and a live dashboard.

## Current milestone

A deterministic exchange library and runnable CLI demonstration. This milestone matches and reserves capacity using simulated microcredits. It does not execute jobs, charge real money, or connect GPUs.

## Run

Install a stable Rust toolchain, then run:

```sh
cargo run -p exchange-cli
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

The demo registers three workers with prices 8, 3, and 3. Jobs 1 and 2 choose the two cheaper workers in offer arrival order. Job 3 cannot afford the remaining worker and queues. Job 4 skips that queued job and reserves the remaining worker. Cancelling job 3 releases its hold. An unfunded request is rejected without changing exchange state.

Expected final buyer balances, in simulated microcredits:

| Account | Available | Reserved |
| --- | ---: | ---: |
| 1 | 970 | 30 |
| 2 | 890 | 110 |

Providers have zero balances because execution and settlement are future work.

## Market rules

* One capacity class: `demo-compute`.
* Each worker may register exactly one offer representing one execution slot.
* Jobs reserve maximum price times duration when submitted, even when queued.
* A submission needs enough available funds for that maximum reservation, even if a cheaper offer already exists.
* Queued jobs are considered by arrival order. An incompatible job does not block later jobs.
* A job matches the cheapest available offer at or below its maximum price; equal prices use offer arrival order, not IDs.
* Matching holds offer price times duration and immediately releases the difference from the original hold.
* New offers and jobs trigger matching. Matching is immediate, so a later cheaper offer does not replace an existing reservation.
* Only queued jobs can be cancelled, and only with the matching buyer ID.
* Prices, funding amounts, and durations must be positive integers. All money is simulated.
* Total funding is bounded by u64::MAX; overflow is rejected.
* Timestamps are supplied by the caller and must be nondecreasing. They are logical metadata, not execution deadlines.
* Identifiers cannot be reused. Worker offers are not renewable in this milestone.
* Reserved jobs remain reserved until settlement and execution are implemented.

## Design and limitations

`exchange-core` contains typed commands, events, state, and read APIs. State mutation requires exclusive access through `&mut self`. Read APIs expose immutable references.

Commands run against a cloned candidate state and replace live state only on success. This deliberately favors simple atomicity over performance. No throughput or latency claims are made. Matching scans queued jobs and available offers; this is not yet an optimized order book.

Tests cover priority, funds conservation, queued holds, cancellation, duplicate IDs, capacity exclusivity, deterministic command execution, timestamp validation, and arithmetic overflow.

Events are in memory notifications in milestone 1, not a durable replay format. Durability will require complete versioned events and transaction boundaries. Buyer IDs are supplied by a trusted local caller; there is no authentication or public API yet.

## Roadmap

1. Deterministic matching core and CLI demo (this branch).
2. SQLite event journal, idempotent commands, balanced ledger, and settlement.
3. Rust server and workers, bounded queues, leases, fencing, retry, and CPU execution.
4. React and TypeScript dashboard driven by real backend state.
5. Optional real GPU inference workers.
6. Kubernetes, Terraform, deployment workflows, observability, and measured benchmarks.

No cloud resources are required for the current milestone.
