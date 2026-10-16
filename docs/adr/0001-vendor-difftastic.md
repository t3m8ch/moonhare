# Vendor difftastic and call it in-process

moonhare computes diffs with difftastic, but difftastic ships only as a binary: it has no library API (upstream issue #216 is open and the maintainer does not plan one), and its JSON output is explicitly unstable and leaves out unchanged text and the highlighting of unchanged tokens. We vendor a snapshot of difftastic (MIT) under `vendor/difftastic`, add a thin `lib.rs`, and call the engine in-process, so one parse yields both the structural diff and the highlighting of the whole file, and the tool installs as a single binary.

## Considered Options

- Run `difft --display json` as a subprocess: cheapest to start, but ties us to an unstable schema and to whatever `difft` version is on `PATH`, and unchanged-token highlighting would need a second engine.
- Build on `syndiff` (a rewrite inspired by difftastic): a different algorithm with different results, not difftastic.

## Consequences

- Cold builds compile dozens of tree-sitter grammars from C and are slow; the binary is large.
- We own the fork. Upstream changes (new languages, algorithm changes) are merged by hand and rarely, and the vendored `LICENSE` must stay.
