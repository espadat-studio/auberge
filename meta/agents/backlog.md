# Backlog: GitHub

Issues and PRDs for this repo live as GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. Use a heredoc for multi-line bodies.
- **Read an issue**: `gh issue view <number> --comments`, filtering comments by `jq` and also fetching labels.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

Infer the repo from `git remote -v` — `gh` does this automatically when run inside a clone.

## When a skill says "publish to the backlog"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`.

## Rejected enhancements

`.out-of-scope/` holds one Markdown file per rejected **concept**, not per issue: why it was rejected, and every issue that asked for it. Triage reads `.out-of-scope/*.md` before evaluating a new request, so a concept settled once is not re-litigated.

Write here only when an **enhancement** is closed `wontfix`. Never for a bug, and never for something closed because it is already implemented — a built feature recorded as a rejection poisons the dedup read. Point those at where the feature lives instead.

Changed your mind? Delete the file. Old issues stay closed as the historical record.
