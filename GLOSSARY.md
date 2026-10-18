# moonhare

A terminal tool for reviewing large changes written by coding agents: browse a Range of commits as structural diffs, track what has been reviewed, and turn inline Comments into a Prompt for the Author agent.

## Language

Every term has a Russian equivalent (`_Russian_`) used in tickets and discussion. Code and docs use the English term.

### Reviewing

**Range**:
The ordered sequence of commits under review, from a base up to a head. A branch names a Range by implying its base.
_Russian_: диапазон
_Avoid_: PR, changeset

**Scope**:
A contiguous run of commits within the Range whose combined change is currently displayed, from the whole Range down to a single commit.
_Russian_: срез
_Avoid_: view, selection

**File change**:
The modification one commit makes to one file. The unit that is marked reviewed.
_Russian_: изменение файла
_Avoid_: patch, hunk (a sub-part of a File change)

**Fold**:
A long stretch of unchanged lines in a diff shown as one placeholder row with its line count. The Reviewer can reveal its lines, a chunk at a time when it is long, and fold them back.
_Russian_: свёрнутый участок
_Avoid_: collapsed region, gap

**Reviewed**:
A File change is reviewed once the Reviewer marks it. A file counts as reviewed in a Scope when every one of its File changes in that Scope is; marking a file in a Scope marks all of its File changes in that Scope.
_Russian_: отревьюено
_Avoid_: viewed, done

**Review**:
The persistent record of working through one Range: which File changes are Reviewed, the Comments left, and the analysis produced.
_Russian_: ревью
_Avoid_: session

**Round**:
The span between two consecutive Prompt copies. Commits that appear after a Prompt is copied belong to the next Round, as the Author agent's response to it.
_Russian_: раунд
_Avoid_: iteration

**Group**:
A set of changed files in the Range that belong together logically, named and described by the Analyst. Every changed file is in exactly one Group, and Groups have a recommended reading order.
_Russian_: группа
_Avoid_: cluster, category, tag

**Summary**:
The Analyst's account of what a Range changes and why.
_Russian_: саммари
_Avoid_: walkthrough (that is CodeRabbit's), overview

**Diagram**:
A sequence diagram or flowchart the Analyst draws, as Mermaid text, for a Group, a file or a fragment, or inside a Summary when the interaction of components changed. moonhare draws it with box-drawing characters; the Mermaid source can always be copied.
_Russian_: диаграмма
_Avoid_: picture, chart, graph

**Explanation**:
The Analyst's answer, on demand, to "what does this hunk or file do and why". Unlike the Summary it is never built in the background.
_Russian_: объяснение
_Avoid_: comment, annotation

**Mechanical**:
Marks a file or Group whose changes need no close reading, such as generated files, lockfiles or bulk renames.
_Russian_: механическое
_Avoid_: boilerplate, trivial

### Hosting

**Forge**:
The code-hosting service that holds the repository and its pull requests. GitHub is the only supported one for now; others (GitLab, Forgejo) can be added later.
_Russian_: форж
_Avoid_: host, provider, platform

### Roles

**Reviewer**:
The human reviewing the Range.
_Russian_: ревьюер
_Avoid_: user (except in the sense of the tool's operator), the Analyst

**Author agent**:
The coding agent that wrote the Range's commits and receives Prompts.
_Russian_: агент-автор
_Avoid_: agent (ambiguous with the Analyst), coder

**Analyst**:
The LLM that moonhare itself runs to analyze a Range for the Reviewer.
_Russian_: аналитик
_Avoid_: reviewer (that is the human), agent

### Feedback

**Comment**:
A note the Reviewer writes, anchored to lines of code in a diff, to a whole file, or to the whole Range, and addressed to the Author agent. It is open until copied into a Prompt, then sent, and the Reviewer closes it once dealt with. It never leaves the machine except inside a Prompt.
_Russian_: комментарий (состояния: открыт, отправлен, закрыт)
_Avoid_: review comment (that is GitHub's), note

**Anchor**:
What a line Comment remembers of its code (the lines, their surroundings and whether they were unique) so it can be found again when lines move. A Comment whose Anchor no longer matches is hidden, not lost.
_Russian_: привязка
_Avoid_: position, line number

**Prompt**:
Text assembled from one or more Comments and copied to the clipboard to hand to the Author agent. Copying code, with or without its Comments, is not a Prompt.
_Russian_: промпт
_Avoid_: export, handoff
