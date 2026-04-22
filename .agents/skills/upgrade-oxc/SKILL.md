---
name: upgrade-oxc
description: "Upgrade oxc, run codegen, and fix any breaking changes."
---

# Upgrade OXC

CRITICAL: Run each step sequentially ONE AT A TIME. Wait for each command to FULLY COMPLETE before proceeding to the next step. DO NOT run multiple commands in parallel - they have dependencies on each other.

## Steps

1. `git checkout main && git pull origin main`
2. Go inside `submodules/oxc`, pull its latest changes.
3. Upgrade the `oxc_` prefixed crates.
4. `cargo check` - if there are errors, fix all breaking changes before proceeding. Common breaking changes include renamed types, changed method signatures, or removed APIs. Study the error messages carefully and update the code accordingly. Use the `submodules/oxc` folder for this, you can run git commands to go back in history and understand breakages made.
5. `git status --short && git diff --stat` - verify expected files changed
6. Summarize the upgrade by reporting: (a) the old and new versions, (b) number of files changed, (c) any breaking changes that were fixed, and (d) notable changes in the diff
