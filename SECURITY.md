# Security Policy

## Reporting a Vulnerability

Report vulnerabilities through GitHub's private vulnerability reporting:
[open a report](https://github.com/soundtrackyourbrand/graphox/security/advisories/new).
This keeps the details private until a fix is released. Please do not open a
public issue for a security report.

We aim to acknowledge a report within five working days and to keep you updated
as we work through it.

## Scope

Graphox is a developer tool: a language server, a code generator, and build-tool
plugins that run against your own schema and documents. The most relevant
concerns are its position in a build — the CLI and the plugins run in other
people's CI — and anything that lets schema or document content escape the
configured output directory, execute code, or reach the network.

## Supported Versions

Fixes land in the latest release. There are no maintained release branches.
