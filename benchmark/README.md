# Benchmarks

Benchmark @oxc-solid-js/compiler against babel-preset-solid.

## Setup

Clone the test repositories:

```bash
# OpenTUI (154 tests)
git clone https://github.com/sst/opentui.git benchmark/opentui
cd benchmark/opentui && pnpm install && cd ../..

# Solid Primitives (800+ tests)
git clone https://github.com/solidjs-community/solid-primitives.git benchmark/solid-primitives
cd benchmark/solid-primitives && pnpm install && cd ../..
```

## Run Benchmarks

```bash
# Pure transform speed benchmark (validation off)
bun run benchmark/bench.ts

# Pure transform speed benchmark (validation on)
bun run benchmark/bench.ts --validate

# Run OpenTUI tests with OXC
cd benchmark/opentui/packages/solid
bun test --preload ./scripts/preload-oxc.ts

# Run Solid Primitives tests with OXC
cd benchmark/solid-primitives
pnpm vitest run -c ./configs/vitest.config.oxc.ts
```
