# Intent: <short title>

<!--
Copy this file to docs/intent/NNN-<slug>.md before starting nontrivial work,
fill it in, and get Layer 1 agreed before you implement. Link it in the PR
description — this is the contract your PR will be reviewed against.

Run the interview first (skills/design-alignment step 1). Do not draft this
from a one-line idea.

Skip the whole thing only for typos and obvious small fixes.
-->

Status: draft | agreed
Agreed with: <reviewer/maintainer>
Date: <YYYY-MM-DD>

## Layer 1 — human-owned, code-free

<!-- No file paths, no function names. A non-coder should be able to
approve this. -->

### Motivation

Why this work exists. The problem, not the solution. This is the implicit
context that silently shapes implementation choices — make it explicit.

### Task

What will change, in behavior terms.

### Completion criteria

How we know it is done. Observable outcomes.

### Non-goals

What this deliberately does NOT do. This is the drift fence.

### Open questions

Anything still unresolved. Delete the section if there are none.

## Layer 2 — implementation sketch (as of <commit-sha>)

<!--
Pin the commit. Code changes between planning and implementation, and stale
instructions get followed off a cliff. If significant time passes before you
start, re-verify this against current code first.
-->

- Rough plan: which areas change and how.
- Known gotchas discovered while scoping.
