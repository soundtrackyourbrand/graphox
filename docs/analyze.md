# Analyzing

`graphox analyze` inspects how a workspace uses GraphQL. Its tools are reading
tools, not gates: nothing they find fails the command, though asking for a scope
that matches nothing — an `--app` naming no project — is still an error. They
are meant to be run by hand while deciding what to change.

| Tool | Question it answers |
|------|---------------------|
| [`analyze selections`](#selections) | What is written more than once, and should be a fragment? |
| [`analyze usage`](#usage) | What does the workspace actually select, and who selects it? |
| [`analyze operations`](#operations) | What does each operation cost the server to answer? |
| [`analyze codegen`](#codegen) | What does the generated TypeScript weigh, and which definition wrote it? |

---

## Selections

Reports selections that recur across a workspace's operations and fragments.

```bash
graphox analyze selections
graphox analyze selections --type Account --limit 0
graphox analyze selections --kind matches_fragment
graphox analyze selections --json | jq '.findings[] | select(.scope == "cross_project")'
```

## What it looks for

The unit of comparison is a **selection set**, keyed by the type it sits on. Two
selections are the same only if they serialize the same, so a field selected
with different arguments, a different alias, or a different sub-selection is a
different thing. `image { placeholder }` never merges with
`image { sizes { thumbnail } }`.

Three kinds of finding:

| Kind | Meaning |
|------|---------|
| `matches_fragment` | A selection set is exactly a fragment that already exists |
| `extends_fragment` | A selection set contains everything a fragment selects, plus more |
| `new_fragment` | A group of fields recurs across definitions with no fragment for it |

The first two are about drift. A field added to the fragment reaches every
spread and silently misses every hand-rolled copy, so one occurrence is already
worth knowing about. The third is about shape: it needs a threshold, because a
group shared by two definitions is a coincidence and a group shared by twenty is
a missing fragment.

## Scope

Analysis runs once per schema, across every project that uses it — two projects
on different schemas could never share a fragment. Each `new_fragment` finding
is tagged:

- **in-project** — every site is in one project, so the fragment can be
  extracted where it is.
- **cross-project** — sites span projects, so extracting means putting the
  fragment somewhere both can import from.

## Thresholds

`--min-fields` (default 3) is how many members a selection must share, and
`--min-uses` (default 3) is how many distinct definitions must share them.

Fields that `rules.required_fields` mandates do not count toward `--min-fields`.
They are still part of the reported group — a fragment selecting them is
correct — but a group of nothing but `id` and `permissions` describes what
graphox inserted, not how anyone wrote the query, and counting them makes every
type in the schema look like a finding.

This follows each project's own `required_fields`, which a project can override
or switch off. A field is discounted only where every project selecting it has
it imposed; where one project leaves it to the author, it describes the
selection again.

## Reading the output

Groups on one type overlap by nature: `{ id name }`, `{ id name kind }` and
`{ id name kind slug }` all describe one duplication. The default output leads
with the widest-reaching shape per type and counts the rest as related shapes.
Pass `--type` to see every shape on a type, or `--json` for all of them.

`--limit` shapes that view only. `--json` always emits every finding, plus an
`unparsed` array of files whose GraphQL did not parse against the schema — a
consumer needs to know the analysis was incomplete, and truncating it to a
display default would drop findings nobody asked to drop.

A shape a fragment already covers is not reported: it is not a missing
fragment, it is one being re-inlined, which `matches_fragment` describes.

Only maximal groups are reported at all: a group is dropped when a larger group
covers the same definitions, since the larger one says everything the smaller
one did.

---

## Usage

Reports what the workspace selects, and by whom. The schema says what *could* be
selected; this says what is — the question behind "who breaks if I change this
field?" and "does anything still use this?".

```bash
graphox analyze usage                                    # types, most-used first
graphox analyze usage --type SoundZone                   # its fields, by consumer count
graphox analyze usage --type SoundZone --field device    # what selects that field
graphox analyze usage --app apps/business --type Account # scoped to one app
graphox analyze usage --unused                           # declared, never selected
graphox analyze usage --json | jq '.fields[] | select(.consumer_count == 0)'
```

### Consumers

A **consumer** is a definition — an operation or fragment — whose own body
selects the field. Usage is counted directly, not transitively: an operation
that spreads a fragment is not a consumer of that fragment's fields, because the
definition you would edit to stop selecting a field is the one that names it.
Selecting the same field twice in one definition still counts once.

### Scoping

- `--app <substring>` (alias `--project`) matches against each project's include
  path, so `--app apps/business` reaches every project under that app. A
  definition outside the scope is not a consumer for that run, so the counts
  read as usage *within* the app.
- `--type <Type>` lists that type's fields with the number of consumers each,
  most-used first, including the ones nothing selects.
- `--field <field>` lists the consuming definitions and their files.

Several schemas usually share types. A schema that merely declares the type
being asked about, and selects nothing on it, is skipped rather than printed as
a block of zeroes — unless `--unused` is what you asked for.

### Unused fields

`--unused` reports fields the schema declares that nothing in scope selects. It
answers "can this be removed" only as far as this workspace goes: another client
against the same schema is invisible here, so treat it as a shortlist rather
than a verdict.

Only fields a selection can name are reported. Input object members are left
out: they appear in argument values, which this does not read, so reporting them
would mark every input field in the schema unused. A file whose GraphQL did not
parse is called out, since it contributes no consumers and would otherwise make
a selected field look unused.

`--limit` shapes the terminal view only. `--json` always emits every matching
field, plus an `unparsed` array of files whose GraphQL did not parse.

---

## Operations

Reports what each operation asks the server for. A gateway rejects on depth and
charges on breadth, so this ranks operations by the properties of the request
they send rather than of the source they were written as.

```bash
graphox analyze operations                               # deepest first
graphox analyze operations --sort lists                  # most-multiplying first
graphox analyze operations --name AccountOverview        # explain one
graphox analyze operations --kind subscription
graphox analyze operations --app apps/business
graphox analyze operations --json | jq '.operations[] | select(.list_nesting >= 3)'
```

### What it measures

| Metric | Meaning |
|--------|---------|
| `depth` | Longest chain of nested fields in the request |
| `own_depth` | The same with spreads left unfollowed |
| `fields` | Fields the response contains, and so resolver calls the request costs |
| `list_nesting` | List-typed fields along one path, with `list_path` naming it |
| `root_fields` | Top-level fields, which the server resolves in parallel |
| `variables`, `spreads` | Declared variables; fragments the request pulls in |

The two depths are a pair on purpose. An operation deep in its own body is one
you rewrite; one whose depth arrives through a fragment is one where you rewrite
what it spreads, and the numbers say which before you open the file.

`list_nesting` is the highest-signal column. Each list-typed field on a path
multiplies the size of the response, so three of them make it cubic in page
size — the shape behind a request that is fine in development and times out on
a large account. Each wrapper counts, so `[[Cell!]!]!` is two.

### Collected, not counted twice

The unit is the response, so selections are collected through spreads and
inline fragments into the shape one reply takes, keyed by response key, as the
spec collects fields. Two spreads that both select `id` on the same object
describe one entry in the response and one resolver call, so they count once; an
alias is its own entry, so it counts separately.

This is the maximum response shape rather than a per-request estimate: two
inline fragments on different type conditions both contribute their fields,
though only the branch matching the concrete type resolves at runtime.

### Scope

Analysis runs once per project, not once per schema. Which fragment a spread
resolves to is a property of the project doing the resolving — a fragment
reaches another project only when it is `@public`, and two projects with
overlapping `include` patterns each resolve the name to their own copy — so an
operation is measured against the fragments its own project can see.

An operation whose spread did not resolve is marked `partial`, `*` in the
terminal and `"partial": true` in JSON. Its numbers are lower bounds: without
saying so, an operation whose fragment is missing would read as a cheap one.

`--limit` shapes the terminal view only. `--json` always emits every matching
operation, plus an `unparsed` array of files whose GraphQL did not parse.

---

## Codegen

Reports what the generated TypeScript weighs and which definition it came from —
the question behind "why is this 3 MB?".

```bash
graphox analyze codegen                                  # heaviest definitions
graphox analyze codegen --sort ast                       # by what reaches the bundle
graphox analyze codegen --kind fragment
graphox analyze codegen --app apps/business
graphox analyze codegen --json | jq '.projects'
```

It generates the same output `codegen` writes and measures it instead of
writing it, so the figures describe the real files. Nothing is written: a run
that only reports numbers leaves no output directory behind.

| Metric | Meaning |
|--------|---------|
| `generated_bytes` | TypeScript the definition contributed: its types, its document AST, and any hooks |
| `ast_bytes` | The `DocumentNode` export alone, out of `generated_bytes` |
| `spread_by` | Definitions that spread this fragment. `null` for an operation |

The split matters because the two halves have different fates. Types are erased
when the app is built; the document AST is data and ships. Sorting by `ast`
therefore ranks by what the bundle pays for, which is a different order from
total size — and with `generate_ast_for_fragments` on, fragments are part of
that bill rather than types alone.

### Fragments keep their own weight

A fragment is generated once and imported by everything that spreads it, and its
bytes stay charged to the fragment rather than being added to each spreading
operation. Attributing them outwards would count one fragment many times and
break the property that makes the numbers worth reading: per-definition bytes
plus each file's shared preamble come to the bytes on disk. `spread_by` carries
the leverage instead — an 8 KB fragment spread by twelve definitions is a
different proposition from the same 8 KB spread by one.

### Reading the totals

The header counts the whole scope, and `shared_bytes` per project is output no
definition accounts for: the per-file preamble, the helper types and the import
lines. Those describe whole files, so they are reported only for an unfiltered
run — under `--kind` the definitions in a file are no longer all in scope and
the sum would no longer be its size.

Fragments shared between projects are counted in each, since codegen emits a
copy into each project that resolves them.

A file the generator rejected is called out and contributes nothing, so its
project would otherwise read as one with less output than it has.
