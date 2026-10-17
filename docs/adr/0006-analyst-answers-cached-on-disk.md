# Cache the Analyst's answers on disk, keyed by what they were asked about

The Summary, the Groups, the Explanations and the diagrams are kept in `.git/moonhare/reviews/<id>/analyst/`, one file per answer, so that reopening moonhare on the same commits spends no tokens. The directory is private to the clone, like marks and Comments, and `:reset` leaves it alone.

## Key

A key is a list of the things the answer depends on, plus a format version: Summary and Groups by base, head and language; a diagram by base, head, language and its subject (file, fragment or Group with its files); an Explanation by language and the code itself, a hunk by its text and a file by its blob at both ends of the Scope. Effort and model are not in the key: they change how an answer is made, not what it is about, and a Summary made on `low` still saves the tokens of one on `max`. `:analyze` is how the Reviewer asks for a better one.

New commits or a rewritten history move the head and so make a new key; the old entries stay until they are evicted and the existing "stale" Summary logic keeps working off the in-memory list. Raising `FORMAT_VERSION` retires every entry when a prompt or the shape of an answer changes.

## Format

A file is a header line (`moonhare-analyst-cache <version>`), the key, and the answer as JSON. The file is named by a digest of the key and the stored key is compared on reading, so a digest collision is a miss. It is written to a temporary file and renamed. A file that is damaged, from another version, or whose JSON does not fit the answer is silently a miss and is overwritten by the next answer. A failure to write is ignored. Past 200 entries the least recently written are deleted, the same idea as the in-memory list, which the cache sits beside rather than replaces.

## Considered Options

- One file for all answers, like marks: one rename per save, but every answer rewrites every other, and one damaged byte loses all.
- Key by the Range's commit list: stricter, but a rebase that changes nothing in the code would miss.
- Include effort and model in the key: a Summary per setting, spending tokens on each change that the Reviewer made to save them.

## Consequences

- Nothing ties an Explanation of a hunk to the rest of the code around it, or to the pull request text at the time; it is reused wherever the hunk reappears.
- Two moonhare processes on one Review may write the same entry: the last writer wins, and both are valid.
- The session tells the interface whether the Summary, Groups or diagram on screen came from the cache (`summary_from_cache`, `groups_from_cache`, `diagram_from_cache`).
