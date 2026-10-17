# Store Comments beside the Reviewed marks, anchored by code

Comments are the Reviewer's own words, so unlike marks they cannot be recreated by looking at the diff again. They live in the same per-Review directory as the Reviewed marks (`.git/moonhare/reviews/<id>/comments`), never shared between clones, and a Review reset (`:reset`) forgets marks only. Deleting Comments is a separate command, `:comments clear`, which asks first.

A line Comment is bound to a side (old or new) and carries an Anchor: a snapshot of the lines it was written on, up to two lines of surroundings, and whether the snapshot was unique in its file. Line numbers alone drift as the Author agent rewrites the branch. A unique snapshot is found wherever it moved; a common one (`}`, a blank line) is found only where its surroundings match too. When no place can be trusted the Comment is not hung under a guess: it is counted as hidden ("N скрытых Comments") and kept.

## Format

One file, records separated by NUL, fields of a record separated by `0x1f`: id, state (`open`/`sent`/`closed`), kind (`L` lines, `F` file, `R` Range), path, side (`o`/`n`), first line, last line, text, snapshot, lines before, lines after, `u` if unique. Lines inside a field are joined by `\n`. The file is replaced through a temporary file and a rename. Records this version cannot read are kept verbatim and written back, so a newer or damaged file is never silently emptied.

## Considered Options

- Delete Comments with the marks on reset: simple and matches the glossary (both belong to the Review), but loses text the Reviewer typed with no way back.
- Nearest match of the snapshot: always finds something, but hangs a Comment under an unrelated `}` after the real code is gone.
- JSON: nicer to read, but needs a dependency or escaping code for what is a flat list; the format is private to a clone and can be replaced later.

## Consequences

- There is no locking between two moonhare processes on one Review: the last writer wins.
- Pins under `refs/moonhare/` are not needed for Comments: the snapshot is stored with them.
