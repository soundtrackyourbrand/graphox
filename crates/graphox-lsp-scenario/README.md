# graphox-lsp-scenario

Replays a working session against the real `graphox lsp` binary on a real
repository, and reports what each step costs the server and whether the server
goes quiet afterwards.

The Criterion benches under `benches/` time single functions. This binary
covers what they cannot: work that only appears when the whole server runs
over time, such as a pass repeated on every poll, a feedback loop between
codegen and file watching, or a change that never finishes settling.

## Running

```sh
make lsp-scenario REPO=path/to/workspace
make lsp-scenario REPO=path/to/workspace ARGS="--steps branch-switch,pull --repeat 3"
```

The target builds the server with the `profiling` Cargo profile, which is
release code that keeps its symbols, so stack samples name graphox's functions.

`REPO` must be a git repository with `graphox.yaml` at its root. The scenario
runs in a clone under `target/lsp-scenario/repo` and never touches `REPO`
itself. Each run starts from the revision given by `--rev` (default `HEAD`)
with untracked and ignored files removed, so the first step always starts
without codegen output on disk.

`--help` lists every option.

## What a session does

The client behaves like VS Code's language client, because the server reacts
to how it is driven:

- Diagnostics are pulled. The workspace report is re-requested 2s after each
  response, and a `workspace/diagnostic/refresh` re-pulls open documents and
  restarts the workspace pull.
- Typing happens one keystroke at a time. Each identifier character requests
  completion and cancels the previous request, and each pause requests
  diagnostics, semantic tokens, symbols, folding ranges, code actions and
  highlights.
- Renames are applied the way refactoring auto-save applies them. Every
  touched file is opened, edited, written and saved, and files that were not
  already open are closed again.
- Open documents are reloaded from disk after git changes them.
- File system events are delivered as `workspace/didChangeWatchedFiles`, but
  only for the watchers the server registered, with string globs matched
  against absolute paths. Watchers that match no file are listed at startup.

Steps, run in this order after startup unless `--steps` says otherwise:

| step | what it does |
| --- | --- |
| `open-files` | opens the edited files and some consumers of the shared fragment |
| `type-operation` | types a field into an operation in the busiest project, saves, undoes, saves |
| `type-fragment` | the same in the most-spread fragment |
| `rename-fragment` | renames that fragment across the workspace, then renames it back |
| `move-file` | renames an open file, then renames it back |
| `external-edits` | edits files on disk, then `git stash`, `git stash pop` and `git checkout .` |
| `branch-switch` | checks out the recent branch whose diff touches the most GraphQL-bearing files, and back |
| `pull` | rewinds `--pull-commits` commits, then checks out the base again as a pull would |
| `rebase` | commits edits on a branch from that far back and rebases it onto the base |
| `schema-change` | types a new type into the most-used schema and saves, then reverts it with git |
| `idle` | does nothing for `--idle-step-secs` |
| `restart` | restarts the server with codegen output and schema cache warm |

The edited operation, fragment and schema are picked from the repository with
graphox's own config and parser, so the steps adapt to any workspace.

## Reading the report

Each step is timed from the start of its action until the server has stayed
under `--threshold` cores for `--quiet-secs`. The server is then measured over
an idle window. CPU is read from the OS, so it covers every thread.

| column | meaning |
| --- | --- |
| `action` | how long the step's own actions took |
| `settle` | time from the end of the action until the server went quiet; `!cpu`, `!progress` or `!watcher` means it had not after `--timeout-secs` |
| `cpu` | CPU seconds from the start of the step until it settled |
| `peak` | the busiest second, in cores |
| `idle` | cores used in the idle window after settling |
| `watch(out)` | watched-file events delivered, and how many of them were for codegen output |
| `pub`, `pulls`, `rfsh` | pushed diagnostics, workspace diagnostic pulls, diagnostic refresh requests |

A step that does not settle has its stacks sampled for `--sample-secs` with
the `sample` tool on macOS. `--sample-all` samples every step from the start
of its action instead, to see where the work in a step goes that does settle.
Sampling pauses the server briefly and often, so timings from a `--sample-all`
run are not comparable with those from a run without it. The report lists
where the samples were written.

Each run writes `report.json` and `session.log` under
`target/lsp-scenario/runs/<timestamp>/`. The log interleaves step markers,
server log messages, stderr and every watcher batch in one timeline. The JSON
has per-step message counts and request latencies, for comparing runs.

Process CPU and memory are read on macOS and Linux.
