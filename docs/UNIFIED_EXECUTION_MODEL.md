# Ciroach Unified Execution Model Specs

The Ciroach UEM is a three-tier execution strategy designed to balance **organizational clarity**, **massive scale**, and **computational efficiency**. It process flat configuration file into a prioritized execution graph.

## The Three Layer

### Tier 1: The Generation Layer (Matrix)

- **Purpose:** Horizontal scaling and cross-environment testing.
- **Mechanism:** Before execution begins, any step containing a `matrix` definition is "exploded".
- **Expansion:** If a step $S$ has a matrix $M$ with $N$ values, $S$ is by $S_1, S_2, ... S_N$.
- **Injection:** Variables from the matrix are injected into the image tags and environment variables using `${{ variable }}` syntax.

### Tier 2: The Orchestration Layer (Stages)

- **Purpose:** High-level synchronization and safety barriers.
- **Mechanism:** Stages are defined in a strict global order (`stages_order`).
- **The Barrier:** A Stage $N+1$ can't begin its first step until every step in Stage $N$ has completed successfully.
- **Fail-Fast:** If any step in Stage $N$ fails, all subsequent Stages are skipped.
- **Transparent Execution:** If a stage is defined in the order but contains no steps, it is **skipped with a warning**.
  - If a stage contains a step but missing from the global order, the pipeline **fails validation**.

### Tier 3: The Execution Layer (DAG)

- **Purpose:** Intra-stage performance optimization.
- **Mechanism:** Within a signle stage, steps run in parallel by default unless a `needs` dependency is specified.
- **Dependency Resolution:** A Step with `needs: ["X"]` will wait until Step $X$ (within the same stage) is finished before starting its container.
- **Concurrency:** Steps with no dependencies or whose dependencies are met run immediately.
- **Scope:** Dependencies cannot cross stage boundaries (Stage barriers take precedence)

## Execution Workflow

1. **Ingestion:** Load `pipeline.toml`.
2. **Expansion Phase:** Identify matrix steps and generate the full set of virtual steps.
3. **Grouping Phase:** Map steps to their respective stages based on the `stage` key.
4. **Topological Sort:** Within each stage, build a dependency graph using the `needs` keys.
5. **Execution Loop:**
   - For `stage` in `stages`:
     - Resolve DAG for current stage.
     - Execution steps via worker pool.
     - Wait for stage completion (The Barrier).
     - Report results.

## Proposed TOML

```toml
# Defines the absolute sequence of stages
stages_order = ["pre-build", "build", "test"]

# Stage: pre-build
[stages.pre-build.steps.lint]
image = "alpine"
command = "ls"

# Stage: build (with Matrix)
[stages.build.steps.compile]
image = "rust:${{ version }}"
matrix = { version = ["1.75", "1.80"] }
command = "cargo build"

# Stage: test (with DAG)
[stages.test.steps.unit-test]
image = "rust:1.80"
needs = ["compile"] # References the step key in the current or previous context
command = "cargo test"
```

## Execution Workflow

1. **Ingestion:** Parse `pipeline.toml` into a `PipelineConfig` Map structure.
2. **Validation Phase:**
   - Cross-reference `stages_order` with the `stages` map.
   - Check for orphaned stages or empty definitions (issue warnings/errors).
3. **Expansion Phase (Planning):**
   - Iterate through stages in order.
   - For each step, check for `matrix`. Generate $N$ `ExecutionStep` objects.
4. **Topological Sort:** Within each stage, organize `ExecutionStep` objects into a dependency graph.
5. **Execution Loop:**
   - For `stage` in `plan`:
     - Launch "ready" steps (those with met dependencies).
     - Stream logs via `mpsc`.
     - Await "The Barrier" (all steps in stage finish).
     - Proceed or Fail-Fast.

## Summary Table

| Layer      | Structure          | Logic Type                  | Role                 |
| ---------- | ------------------ | --------------------------- | -------------------- |
| **Matrix** | `matrix = { ... }` | Multiplicative ($1 \to N$)  | Environment Scaling  |
| **Stage**  | `[stages.<id>]`    | Sequential (Blocking)       | Structural Flow / UI |
| **DAG**    | `needs = [...]`    | Parallel (Dependency-based) | Micro-Optimization   |
