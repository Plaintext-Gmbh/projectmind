# Branding

## The pairing

> **ProjectMind by Plaintext — Your project, explained by AI.**

Use the full pairing in headers, slide titles, and any first-touch surface.
The name is cool and brandable but not self-explanatory; the claim does the
explaining. Together they answer both *"what is it called?"* and *"what does
it do?"* in one line.

`by Plaintext` is the maker attribution — same convention as "Mac by Apple",
"Slack by Salesforce". It places the product inside the Plaintext family
without competing with the product name visually. In secondary positions
(in-app chrome, footer credits, package metadata) the short form
**ProjectMind** is fine; in primary positions, prefer the full
**ProjectMind by Plaintext**.

## Name

**ProjectMind**

A read-only architecture browser that reads a source repository and explains
its structure — modules, classes, relationships, embedded HTML, Markdown — in
a form that both humans and LLM-driven coding agents can navigate.

## Tagline

> **Your project, explained by AI.**

The headline used on GitHub, the README, and any future landing page. It
frames the value proposition for non-technical readers ("AI explains it")
while being short enough for repository metadata. Always paired with the
name in primary positions; the claim alone may be used as a subtitle in
long-form copy where the name appeared just above it.

## GitHub repository description

> ProjectMind uses AI-ready project maps to explain software architecture,
> classes, modules and relationships in a way humans and coding agents can
> navigate.

## Voice

Clear, calm, slightly technical. The product is for software engineers and
the agents that increasingly assist them, but the framing is approachable
enough that a project manager or a designer can understand what it does
without owning a Rust toolchain.

Avoid:

- *"Revolutionary"*, *"the future of software"*, generic AI buzzwords.
- Calling the user "you should" — describe what the tool does, not what the
  user must do.

Prefer:

- Concrete capabilities: "renders the bean graph", "lists HTML snippets",
  "highlights line ranges from MCP intents".
- The verb **understand**: "understand your project", "understand a class".

## Alternative taglines (rejected, kept for reference)

| Claim | Effect |
|---|---|
| Let AI explain your project. | Very clear, non-techie friendly. |
| Your project, explained by AI. | Short, strong, **chosen as primary**. |
| AI-powered project understanding. | More professional, slightly technical. |
| Turn code into clarity. | Cool, less direct. |
| From code to clarity. | Strong, elegant. Used as a secondary header in long-form copy. |
| Understand your software through AI. | Clear and broad. |
| AI maps your code, so you can understand it. | Slightly longer but exact. |
| See your software through AI eyes. | Cool, punchy. |
| Make your codebase explain itself. | Very fitting for developers. |
| The AI guide to your software. | Friendly, accessible. |

## Alternative names (rejected)

- **ExplainMyProject** — very non-techie, instantly understandable, but
  less cool. Kept here in case the marketing target shifts.
- **plaintext-ide** — the original working name. Accurate (a "read-only IDE"
  for plaintext code) but didn't communicate the AI angle and read like an
  internal codename.

## Logo

The logo from the `plaintext-ide` working name stays unchanged. It's a
neutral mark, doesn't tie to the old codename visually, and reusing it
keeps continuity with the Plaintext family of tools. No refresh planned.

## Where the name appears

- GitHub repo: `Plaintext-Gmbh/projectmind`
- Cargo crates: `projectmind`, `projectmind-core`, `projectmind-mcp`,
  `projectmind-plugin-api`, `projectmind-lang-*`, `projectmind-framework-*`
- Binary: `projectmind-mcp` (the MCP server) and `projectmind-app` (the
  Tauri shell)
- Tauri product name: **ProjectMind**
- Window title: **ProjectMind**
- MCP server name (in `.mcp.json` snippets): `projectmind`

## Migration note

Until April 2026 this project lived as **plaintext-ide**. References to the
old name in user clones (`.mcp.json` entries, install paths, environment
variables prefixed `PLAINTEXT_IDE_*`) need a one-time update — see the
README's *"Use with Claude Code"* section for the new snippet.
