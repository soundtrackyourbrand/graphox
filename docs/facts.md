# Facts for other tools

With `emit_facts` on, codegen writes `graphql-facts.json` into each output
directory, next to `manifest.json`. It describes every operation and fragment
the directory's `graphql()` function knows: what each one selects, with its
fragments expanded, and the arguments, variables and directives it writes.

graphox does not read the file. It is for tools that check how documents are
used from the code that sends them, which graphox does not see: a linter that
follows a call site to the document it passes, for example.

```yaml
# graphox.yaml
codegen:
  emit_facts: true
```

The option is read from the codegen settings of the output directory, so
projects sharing a directory should agree on it. Turning it off removes the
file at the next codegen.

## Joining a call site to its definitions

A definition is keyed the way the `graphql()` overloads in `graphql.ts` and
the entries of `manifest.json` are: by the source text of the block it was
written in. That text is the whole template literal or `.graphql` file, so
every definition written in one block shares it.

`graphql()` returns one document per block, and `returned` marks the
definition it is: the block's first operation by type name, or, in a block of
fragments alone, its first fragment by name. Its other definitions still have
entries of their own, with `returned: false`. `document` names each
definition's export. A fragment's document is only exported when codegen generates
documents for fragments (`generate_ast_for_fragments` or fragment masking);
otherwise `graphql()` gives a block of fragments its type alone.

## Format

The file is one JSON object. `version` is the version of the format, which
changes whenever a reader of the previous version could misread the file.
`limits` lists the [`field_arguments`](./rules.md#field_arguments) limits of
the directory's project. `definitions` lists the operations, then the
fragments, one definition per line. Keys within an object are sorted.

```json
{"version":1,"limits":[…],"definitions":[
{"document":"PageQueryDocument","kind":"query","name":"Page","returned":true,"selections":[…],"source":"…","type":"Query","variables":[…]},
{"document":"RowDocument","kind":"fragment","name":"Row","returned":true,"selections":[…],"source":"…","type":"Track"}
]}
```

### Limits

One entry per limited argument, ordered by field:

| Key | Meaning |
|-----|---------|
| `field` | The field's schema coordinate, such as `Playlist.tracks` |
| `argument` | The argument's name |
| `max` | The largest number it may be, for a maximum |
| `values` | The values it may be, for a list of allowed values |
| `severity` | `error`, `warning` or `info` |
| `reason` | The reason given, if any |

A field matches a limit when its `parent` and `name` match `field`. graphox
checks literals and variable defaults itself; a reader checks the values a
caller passes for the variables.

The limits are those of the first project writing to the directory, so
projects sharing a directory should declare the same ones; `graphox codegen`
warns when they differ.

### Definitions

| Key | Meaning |
|-----|---------|
| `kind` | `query`, `mutation`, `subscription` or `fragment` |
| `name` | The definition's name; `null` for an anonymous operation |
| `document` | The name of its document export |
| `returned` | Whether `graphql()` returns this definition for its block |
| `source` | The source text of the block it was written in |
| `type` | The root type of an operation, or a fragment's type condition |
| `variables` | Operations only: each variable's `name`, `type` as written, and its `default` value if it has one |
| `directives` | The definition's directives, when it has any |
| `selections` | What it selects |

### Selections

Every selection has a `kind`, and `directives` when it has any.

| `kind` | Keys |
|--------|------|
| `field` | `parent`, the type the field is selected on; `name`; `alias` if it has one; `type`, the field's type in the schema, such as `[TrackEdge!]!`; `arguments` if it writes any; `selections` if the field has a selection set |
| `inline` | `on`, the type condition, if it has one; `selections` |
| `spread` | `fragment`, the fragment's name; `on`, its type condition; `selections`, the fragment's selections, expanded in place |

A spread whose fragment cannot be found, or that would expand a fragment
already being expanded, has `unresolved: true` and no `selections`. Codegen
does not generate documents like that, so a reader should not meet one; the
marker is there so a selection is never left out without saying so.

Only arguments the document writes are listed. An argument it leaves out,
and the default the schema gives it, are not.

### Values

Arguments and variable defaults are written as tagged values, as the document
writes them:

| `kind` | Keys |
|--------|------|
| `variable` | `name` |
| `int`, `float` | `value`, the number as written, as a string |
| `string`, `enum` | `value` |
| `boolean` | `value` |
| `null` | |
| `list` | `values` |
| `object` | `fields`, each with `name` and `value` |

Numbers stay strings so no precision is lost between GraphQL and the reader.
