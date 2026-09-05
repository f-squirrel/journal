---
name: commit
description: Guides writing git commit messages for this repository in Conventional Commits format (type(scope): description), with short, mostly body-less messages. Use this whenever Claude is about to run `git commit`, is asked to commit changes, stage and commit, "save this", or write/suggest a commit message — even if the user doesn't say "conventional commits" explicitly. Also consult it when amending a commit message or squashing commits so the result still conforms.
---

# Conventional Commit Guide

This repository writes every commit message in [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) format. The format exists so that commit history stays scannable and, if this repo ever adds tooling like changelog generation or semantic-version bumps, machine-parseable. Follow it for every commit you create here, not just ones the user explicitly labels as needing it.

## Subject line

```
<type>(<optional scope>): <description>
```

- **type** — one of the standard Conventional Commits types, chosen for what actually changed:
  - `feat` — a new feature for the user
  - `fix` — a bug fix
  - `docs` — documentation only
  - `style` — formatting, whitespace; no code meaning change
  - `refactor` — code change that neither fixes a bug nor adds a feature
  - `perf` — a performance improvement
  - `test` — adding or correcting tests
  - `build` — build system or external dependencies
  - `ci` — CI configuration/scripts
  - `chore` — everything else (tooling, repo maintenance)
- **scope** (optional) — the area of the codebase touched, e.g. `feat(auth): ...`. Skip it when the change is repo-wide or a scope would be forced/meaningless — an omitted scope is better than a padded one.
- **description** — imperative mood ("add", not "added" or "adds"), lowercase start, no trailing period. Say what the commit does, not what problem prompted it.
- **Breaking changes**: append `!` after the type/scope, e.g. `feat(api)!: remove deprecated endpoint`.

Keep the subject line short — aim for 50 characters, hard-wrap at 72. If the change can't be summarized that tightly, it's often a sign the commit should be split rather than that the subject should run long.

## Body: keep it very short, or skip it

Default to a subject-only commit. Most changes are fully explained by a good subject line, and the diff itself is the detail — don't restate what the diff already shows.

Only add a body when there's something a reader genuinely cannot get from the subject or the diff, such as:
- A breaking-change note (`BREAKING CHANGE: ...`) explaining what breaks and what to do instead.
- Non-obvious *why* — a constraint, a rejected alternative, a workaround for something external — that would otherwise need to be rediscovered later.

When you do add a body, keep it to one or two short sentences. Do not write multi-paragraph explanations, bullet-point changelogs, or restate the file list — that belongs in a PR description, not a commit body.

## Examples

**Subject only (the common case):**
```
fix(parser): handle empty input without crashing
```

**With a short body (breaking change):**
```
feat(api)!: require auth token on /export

BREAKING CHANGE: /export now returns 401 without a bearer token.
```

**With a short body (non-obvious rationale):**
```
chore(deps): pin lodash to 4.17.21

Later versions break the CommonJS build used by the CLI.
```

**Wrong — vague type / no imperative mood / needless body:**
```
update: fixed stuff in the parser

This commit changes the parser file to fix a bug where it was
crashing on empty input. Also cleaned up some formatting.
```
This should instead be `fix(parser): handle empty input without crashing`, no body.

## Applying this when committing

When constructing the actual `git commit` command, pass the subject and (if present) body as separate `-m` flags, or use a heredoc — never let a body get invented just to fill space. If a change genuinely mixes unrelated concerns (e.g. a feature plus an unrelated formatting pass), prefer splitting into separate commits over writing one commit with a padded multi-topic body.
