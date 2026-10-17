# Key Reviewed marks by file content, not commit hash

A Reviewed mark is keyed by the content of the File change (the path and the blob ids it produces), not by the commit SHA. Author agents amend, rebase and force-push constantly, which rewrites every SHA: marks keyed by SHA would all be lost on each rewrite, while marks keyed by content survive whenever the resulting file is unchanged. They also record which version of a file the Reviewer actually saw, so the unreviewed view can show only the delta from it. To stop git from garbage-collecting those versions, moonhare pins them with refs under `refs/moonhare/`.

## Considered Options

- Key by commit SHA: simplest and matches git's own history model, but every amend or rebase resets the Reviewer's progress, which defeats the loop where the Author agent keeps iterating on the same branch.
- Key by patch-id: survives a rebase of an unchanged patch, but does not record the version that was reviewed, so the delta after a change cannot be computed.

## Consequences

- moonhare writes refs in the repository's `refs/moonhare/` namespace, the only writes outside its own state directory besides the Analyst's temporary worktree.
- Pins for superseded versions accumulate until the Review is reset.
- Marks are read as edges between versions of a file (old blob to new blob). A file is Reviewed in a Scope when the marks lead from the version at the base of the Scope to the version at its head, so a squash or a re-cut of the commits keeps the marks as long as the Reviewer has been through the same versions. The delta of a changed file starts from the target of the latest mark on that way.
