# Agents for making edits

Claude Code subagents for this repository. One session directs the edit and
does the fix loop itself; helpers take work that would fill its context with
reading or pictures, and hand back file paths and numbers, never images.

| Agent | Model, effort | Does |
| --- | --- | --- |
| `edit-director` | Opus, high | Main session: plans the edit in `out/<slug>/brief.md`, owns `project.json`, runs the fix loop, delegates the rest |
| `reference-analyst` | Sonnet, medium | Analyses a reference video, runs the vision pass (looks at every shot's strip, fills the fixed-vocabulary answers, merges them) and writes notes |
| `picture-curator` | Sonnet, low | Picks, orders and frames pictures; writes `picks.json` |
| `shader-smith` | Sonnet, high | Writes and checks a custom WGSL effect |
| `fit-runner` | Haiku, low | Preview render, compare, `project_fit`, compare again; returns scores |
| `edit-critic` | Opus, medium, read-only | Fixed checklist review before an edit is called done, including the side-by-side sheet against the reference |
| `edit-builder` | Sonnet, medium | Independent work only: a variant in its own file, or shots on separate layers |

Start a session as the director:

```bash
claude --agent edit-director
```

Why this shape and not a pipeline of roles: a chain of planner, builder and
tester loses detail at every handoff, and multi-agent runs use several times
the tokens. Anthropic's measurements favour one capable session for work that
fits in one context, with helpers only where a context fills with reading.
Our `compare` and `fit` already judge the numbers, so the critic checks only
what numbers cannot see. A new `agents/` folder needs a session restart
before Claude Code loads it.
