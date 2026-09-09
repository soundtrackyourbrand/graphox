# Analyzing

`graphox analyze` inspects how a workspace uses GraphQL. Its tools are reading
tools, not gates: nothing they find fails the command, though asking for a scope
that matches nothing — an `--app` naming no project — is still an error. They
are meant to be run by hand while deciding what to change.

| Tool | Question it answers |
|------|---------------------|
| [`analyze selections`](#selections) | What is written more than once, and should be a fragment? |
| [`analyze usage`](#usage) | What does the workspace actually select, and who selects it? |

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
