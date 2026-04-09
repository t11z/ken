# ADR-0005: SQLite via sqlx for server persistence

- **Status:** Accepted
- **Date:** 2026-04-09
- **Deciders:** @t11z
- **Supersedes:** —
- **Superseded by:** —

## Context

The Ken server needs persistent storage for enrolled endpoints, heartbeat history, status snapshots, command queues, audit events, admin sessions, and the server's own administrative secrets. The data set is small in absolute terms — a single household has at most a few dozen endpoints, heartbeats arrive at minute intervals, and historical retention can be aggressive without storage pressure. The deployment target is a Raspberry Pi running Docker Compose, where operational simplicity matters more than horizontal scalability.

The choice of database is foundational: it determines the query interface, the migration tooling, the backup story, the operator's mental model, and the test infrastructure. It must be made before any storage code is written, and once written, changing it is expensive.

## Decision

The Ken server stores all persistent state in **SQLite**, accessed through the **`sqlx`** crate with **compile-time-checked queries** via the `sqlx::query!` and `sqlx::query_as!` macros. The database file lives in the configured data directory (default `/var/lib/ken/ken.db` inside the container). Schema migrations are SQL files in `crates/ken-server/migrations/`, applied at server startup via `sqlx::migrate!`.

The compile-time query checking is enabled, which requires either a live `DATABASE_URL` at build time or a checked-in `.sqlx/` directory. Ken uses the checked-in approach: the project commits the prepared query metadata via `cargo sqlx prepare`, so CI does not need a database connection to compile. When schema changes, the prepared queries are regenerated and committed alongside the migration.

`sqlx::SqlitePool` is the connection pool. Pool size is small (default 5) and configurable. Write operations use single connections; the rest of the pool serves reads. SQLite's single-writer model is acceptable at Ken's scale.

The storage layer exposes a `Storage` struct that wraps the pool and provides typed methods for every query the application needs. Handlers do not see the raw pool. The mapping from database rows to domain types happens inside the storage layer, not in handlers.

## Consequences

**Easier:**
- Zero operational complexity. There is no separate database process to install, configure, secure, back up, or upgrade. The "database" is a file on disk inside the same container as the server. Backing up Ken means copying one file.
- Deployment on a Raspberry Pi is trivial. SQLite has been the most-deployed database engine on every embedded Linux platform for two decades. Ken inherits that stability for free.
- Schema changes go through `sqlx migrate` with versioned SQL files. There is no Object-Relational Mapping mismatch debate, no schema diff tool to learn, no "but the ORM thinks the column should be NULLable" arguments. The schema is whatever the SQL says.
- Compile-time query checking catches a category of bugs that would otherwise show up at runtime: typos in column names, type mismatches between database and Rust, queries that reference dropped columns. These are build errors, not 500s.
- Tests can use an in-memory SQLite database with `sqlite::memory:` and get a fresh schema per test in milliseconds. No fixture management, no test database to provision.

**Harder:**
- SQLite has a single-writer model. Concurrent writes are serialized through one writer at a time. At Ken's deployment scale (heartbeats from tens of endpoints at minute intervals), this is not a bottleneck — but it is a structural constraint that would matter if Ken ever had to scale to thousands of endpoints. We accept this and document it.
- `sqlx`'s compile-time checking imposes a workflow constraint: schema changes require regenerating the `.sqlx/` directory and committing it. Forgetting this step produces opaque CI failures. The convention is documented in `crates/ken-server/CLAUDE.md`.
- SQLite does not have a "real" date-time type. Timestamps are stored as ISO-8601 text and parsed via the `time` crate. This is a minor friction in queries that filter by date range.
- Some advanced SQL features (window functions, CTEs in DDL) work in SQLite but are easy to write incorrectly because the SQLite documentation is leaner than Postgres's. The schema should stay simple enough that these features are rarely needed.

**Accepted:**
- We forgo Postgres's concurrent write performance, its richer type system, and its broader ecosystem of operational tooling. The trade-off is correct for Ken's scale and deployment context. If Ken ever outgrows SQLite, the storage layer's typed interface (no raw SQL escaping into handlers) keeps the migration cost contained — it would be a focused refactor of one module, not a project-wide rewrite.
- Backups and disaster recovery are the operator's responsibility. Ken provides no built-in backup mechanism in Phase 1; the operator copies `ken.db` (and the CA key directory) on whatever schedule they choose. Automatic backup is a future enhancement that does not require an ADR change.

## Alternatives considered

**Postgres.** Rejected because adding a second container to the Docker Compose deployment doubles the operational complexity and the maintenance surface, in exchange for performance characteristics Ken does not need at its scale. Postgres would also need its own backup story, its own auth model, and its own connection management. The Raspberry Pi is not the right place for a full database server.

**An ORM such as Diesel or SeaORM instead of `sqlx`.** Rejected because ORMs hide the SQL behind a query DSL that becomes its own dialect to learn, and they introduce a layer of indirection between the schema and the code that obscures what queries actually run. `sqlx`'s query macros give us compile-time checking *and* visible SQL — we get the safety without the abstraction tax. Diesel is the most mature ORM in Rust, but its DSL is heavier than the queries Ken needs.

**A schemaless or document store such as `sled` or `redb`.** Rejected because Ken's data is fundamentally relational (endpoints have heartbeats, heartbeats reference commands, commands belong to endpoints) and the admin UI's queries are best expressed as SQL joins. A key-value store would force us to maintain indexes by hand and do join logic in application code. The simplicity of the document model is real but it would make the admin UI's data fetching significantly more awkward.

**Embed Redis or another in-memory store for hot state with periodic persistence.** Rejected because Ken does not have hot-state performance requirements that justify the architectural complexity of two stores. Heartbeats are written once a minute per endpoint and read on-demand. SQLite handles this load without breathing hard.
