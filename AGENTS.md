# Zircon — Agent Guidelines

Read and follow [CLAUDE.md](CLAUDE.md) for project constraints and workflows.

## Default implementation: Ponytail

Use the globally installed `ponytail` skill at full intensity for coding tasks.
Read `~/.agents/skills/ponytail/SKILL.md`. Understand the affected flow, reuse
existing code and standard libraries, and make the smallest correct change.
Avoid speculative abstractions and unnecessary dependencies. Preserve requested
scope, correctness, safety, and required verification. Caveman governs response
style; Ponytail governs implementation. Disable when the user requests it.

## Default communication: Caveman

Always use the globally installed `caveman` skill in this repository. Read
`~/.agents/skills/caveman/SKILL.md` and apply it to every response, including
progress updates and final answers, unless the user requests normal mode.

Keep communication terse while preserving technical accuracy, exact code and
error messages, and necessary verification details. Use full sentences when
needed for clarity or safety. This is a communication preference, not permission
to skip required work, tests, or documentation.
