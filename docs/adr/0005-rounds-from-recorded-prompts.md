# Derive Rounds from the Prompts recorded in the Review

A Round is not stored. What is stored is each copied Prompt (`.git/moonhare/reviews/<id>/prompts`, one line per Prompt: number, head commit, number of commits the Range had), and the commit list is cut into Rounds from those records every time it is drawn. Commits at or before a Prompt's recorded point belong to the Round it closed; later ones are the Author agent's answer in the next Round.

The point is found by the recorded head commit; when the Author agent rewrote history and that commit is gone, the recorded commit count stands in for it. A Prompt copied before any new commit arrived adds no Round: it joins the Round that is still empty ("Round 2, after #1, #2"), and a Round without commits has no header of its own.

Every copy gets the next number, which is also written into the Prompt's preamble. That keeps two copies of the same Comment different, because mosh does not forward an identical repeated OSC 52.

## Considered Options

- Store a Round number on every commit: breaks as soon as commits are amended or rebased, which is exactly what the Author agent does in response to a Prompt.
- Store only a timestamp per Prompt and compare with commit dates: dates survive rebases badly and say nothing about what the Reviewer had seen.

## Consequences

- After a rebase that changes the count of commits before the recorded point, a Round boundary may move by a commit. The fallback is a guess, kept simple on purpose.
- Like Comments, the Prompts are private to a clone and are not touched by `:reset`.
