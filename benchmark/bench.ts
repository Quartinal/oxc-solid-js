#!/usr/bin/env bun
/**
 * Benchmark @oxc-solid-js/compiler vs babel-plugin-jsx-dom-expressions.
 *
 * Usage: bun run benchmark/bench.ts [path-to-repo] [--validate]
 */

import { Bench } from "tinybench"
import { transformAsync } from "@babel/core"
// @ts-expect-error
import solid from "babel-preset-solid"
// @ts-expect-error
import ts from "@babel/preset-typescript"
import { transform as transformOxc } from "@oxc-solid-js/compiler"
import { Glob } from "bun"

const args = process.argv.slice(2)
const validate = args.includes("--validate")
const repoPath = args.find((arg) => !arg.startsWith("--")) || "benchmark/solid-primitives"

// Find all JSX/TSX files
const glob = new Glob("**/*.{jsx,tsx}")
const files: { path: string; code: string }[] = []

for await (const path of glob.scan({ cwd: repoPath, onlyFiles: true })) {
  if (path.includes("node_modules") || path.includes("dist")) continue
  const fullPath = `${repoPath}/${path}`
  const code = await Bun.file(fullPath).text()
  if (code.includes("<") && (code.includes("jsx") || code.includes("tsx") || code.includes("return"))) {
    files.push({ path, code })
  }
}

console.log(`Found ${files.length} JSX/TSX files in ${repoPath}`)
console.log(`Validation: ${validate ? "on" : "off"}\n`)

const bench = new Bench({ time: 1000 })

bench.add("Babel", async () => {
  for (const file of files) {
    await transformAsync(file.code, {
      babelrc: false,
      configFile: false,
      filename: file.path,
      sourceFileName: file.path,
      presets: [[solid, { generate: "dom", validate }], [ts, {}]],
    })
  }
})

bench.add("OXC", async () => {
  for (const file of files) {
    transformOxc(file.code, { generate: "dom", filename: file.path, validate })
  }
})

await bench.run()

console.log("Benchmark")
console.table(bench.table())

const babelTask = bench.tasks.find((task) => task.name === "Babel")
const oxcTask = bench.tasks.find((task) => task.name === "OXC")
const babelMean = babelTask?.result?.mean ?? (babelTask?.result?.latency?.mean as number | undefined)
const oxcMean = oxcTask?.result?.mean ?? (oxcTask?.result?.latency?.mean as number | undefined)

if (babelMean == null || oxcMean == null) {
  console.log("\nUnable to compute speed comparison (missing benchmark results).")
  process.exit(0)
}

const ratio = babelMean / oxcMean
const inverseRatio = oxcMean / babelMean
const deltaPercent = ((oxcMean - babelMean) / babelMean) * 100

let summary: string
if (Math.abs(deltaPercent) < 1) {
  summary = "@oxc-solid-js/compiler is about the same speed as babel"
} else if (oxcMean < babelMean) {
  summary = `@oxc-solid-js/compiler is ${ratio.toFixed(1)}x faster than babel`
} else {
  summary = `@oxc-solid-js/compiler is ${inverseRatio.toFixed(1)}x slower than babel`
}

console.log(`\n${summary}`)
