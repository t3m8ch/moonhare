# Run the Analyst through `claude -p`

The Analyst is the unmodified `claude` binary in print mode (`claude -p --json-schema ...`), run under the Reviewer's own login, not ACP, the Agent SDK or an API key. Print mode returns schema-validated output natively and is the pattern Anthropic's docs name as allowed for subscription use; ACP has no structured final result and goes through the Agent SDK, which the terms single out. The Analyst sits behind a seam so other agents (for example over ACP) can be added later.

## Considered Options

- ACP with the Claude adapter: would also give Codex, Gemini and others, but the final answer is free-form streamed text, the Rust client crate shipped four major versions in three months, and it needs a Node adapter.
- Anthropic API with a key: clean terms, but bills separately from the subscription the Reviewer already pays for.

## Consequences

- moonhare is a personal, local tool. It never reads, stores or forwards Claude credentials, and the terms for third-party tools that use a subscription are unclear, so it sticks to the one pattern the docs name as allowed.
- The child process runs with `ANTHROPIC_*` stripped from its environment: in print mode a stray `ANTHROPIC_API_KEY` is always used and would silently switch billing from the subscription to the API.
- The Analyst gets read-only tools only.
