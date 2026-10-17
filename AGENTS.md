## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on `t3m8ch/moonhare`, using the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Language

- **Tickets and PRs are in Russian**: GitHub issues (specs, sub-issues, follow-ups), PR titles and descriptions. Domain terms come from the `_Russian_:` lines of `GLOSSARY.md`. Code identifiers, CLI commands and file paths stay in English.
- **Every term in `GLOSSARY.md` has a Russian synonym** on a `_Russian_:` line right after its definition. The English term stays canonical in code and docs. When you add or rename a term, add the Russian one in the same edit; if the user used their own Russian wording, prefer it.
- If a ticket needs a domain term that has no Russian synonym yet, add it to the glossary first.
