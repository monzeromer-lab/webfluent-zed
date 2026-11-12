# WebFluent for Zed

Language support for [WebFluent](https://github.com/monzeromer-lab/WebFluent)
`.wf` files in the [Zed](https://zed.dev) editor.

- Syntax highlighting, bracket matching and rainbow brackets, auto-indent,
  code folding, outline panel and breadcrumbs, Vim text objects — all from the
  Tree-sitter grammar in `../tree-sitter-webfluent`.
- Diagnostics, completions, hover documentation, go to definition, document
  symbols and quick fixes from `wf-lsp`, the WebFluent language server.
- Snippets for pages, components, stores, resources, style rules and the
  common built-ins. Type `page`, `resource`, `card`, … and accept the completion.
- ```` ```wf ```` fences in Markdown are highlighted as WebFluent.

## Layout

```
webfluent-zed/
  extension.toml          manifest: grammar, language server, snippets
  Cargo.toml, src/        the Rust part, compiled to WebAssembly by Zed
  languages/webfluent/
    config.toml           comments, brackets, indentation, overrides
    highlights.scm        syntax highlighting
    brackets.scm          bracket pairs
    outline.scm           outline panel entries
    indents.scm           auto-indent ranges
    injections.scm        embedded languages (comments)
    overrides.scm         scopes that config.toml refers to
    textobjects.scm       Vim-mode text objects
  snippets/webfluent.json
```

The grammar is not vendored here. `extension.toml` points at
[tree-sitter-webfluent](https://github.com/monzeromer-lab/tree-sitter-webfluent)
and a commit, and Zed clones it and compiles `webfluent/src/parser.c` and
`webfluentx/src/parser.c` itself.

## The language server

The extension looks for `wf-lsp` in this order and uses the first it finds:

1. `lsp.wf-lsp.binary.path` in your **user** Zed `settings.json` (see below).
2. A `wf-lsp` on your `PATH` — `cargo install --path crates/wf-lsp` from a
   checkout, or `cargo install --git https://github.com/monzeromer-lab/WebFluent wf-lsp`.
3. The [WebFluent release](https://github.com/monzeromer-lab/WebFluent/releases)
   this version of the extension pins (`SERVER_VERSION` in `src/webfluent.rs`),
   downloaded on first use (the status shows in Zed's bottom bar). The
   download is the bare `wf-lsp` executable, and its SHA-256 is checked
   against the release's `SHA256SUMS` before it is made executable: a
   download that does not match is deleted, never run. A new server reaches
   you with a new version of the extension, never because a release was
   published.
4. A copy it downloaded and checked before, when the release cannot be
   reached (offline, or rate-limited), so the editor keeps working without
   the network.

To pin a binary, or pass it arguments or environment, in your user
settings (`zed: open settings`):

```json
{
  "lsp": {
    "wf-lsp": {
      "binary": {
        "path": "/home/me/.cargo/bin/wf-lsp",
        "arguments": [],
        "env": {}
      }
    }
  }
}
```

Anything under `lsp.wf-lsp.settings` is forwarded to the server as its
workspace configuration.

### What a project's settings cannot do

A project you open may carry its own `.zed/settings.json`, and Zed hands an
extension the user's and the project's settings already merged — the
extension API (0.7) has no way to ask which file a value came from. A
project that set `binary.path` to `/bin/sh` and `arguments` to `["-c", …]`
would otherwise run a command the moment a `.wf` file opened. So:

- When the project's `.zed/settings.json` says anything about
  `lsp.wf-lsp.binary` — or cannot be read as JSON and mentions `wf-lsp` —
  the `binary` settings are ignored entirely: path, arguments and
  environment, yours included, for that project. The server is then found
  on `PATH` or downloaded as above. A project may still set
  `lsp.wf-lsp.settings`.
- `binary.path` must be an absolute path outside the project. A relative
  path, one with `..`, or one inside the project's folder is ignored, with
  its arguments and environment.

Zed's extension API cannot hash a file itself, so the extension computes the
SHA-256 in its own code (the `sha2` crate) over the file it downloaded, and
only then moves it into place.

## Developing the extension

You need a Rust toolchain installed through `rustup` (Zed adds the
`wasm32-wasip2` target itself) and, for the grammar, `git` on your `PATH`.

1. In Zed, open the command palette and run `zed: install dev extension`,
   then pick this directory.
2. Open any `.wf` file. Zed compiles the Rust crate and the grammar the first
   time, which takes a minute; after that it is instant.
3. After changing a query, a snippet or `config.toml`, run
   `zed: reload extensions`. After changing the Rust code, reinstall the dev
   extension.

Zed writes `target/`, `grammars/` and `extension.wasm` into this directory
while doing so; they are ignored by git.

### Changing the grammar

The grammar Zed uses is whatever `[grammars.webfluent] rev` in
`extension.toml` names, so a grammar change reaches the extension in two
steps: commit the grammar, then point `rev` at that commit and commit the
manifest. From the repository root:

```bash
just grammar-test          # regenerate, run the corpus, parse every .wf in the repo
git commit -am "grammar: …"
just zed-pin-grammar       # rewrites rev = <HEAD> in extension.toml
git commit -am "zed: pin the grammar"
```

To try an uncommitted grammar in Zed before that, point the manifest at your
working copy for the duration — `repository = "file:///path/to/WebFluent"`
still needs `rev` to be a commit that exists locally, so commit the grammar
on a branch first — and change it back before committing the manifest.

`just zed-check` builds the Rust crate for `wasm32-wasip2`, validates every
query against the grammar, and checks that the modifier vocabulary in
`highlights.scm` matches the compiler's.

### Troubleshooting

- **No highlighting**: open `zed: open log`. A query that names a node the
  grammar does not have fails there with the file and line.
- **The server does not start**: the error in the log names every place the
  extension looked. Installing `wf-lsp` on the `PATH` is the quickest fix.
- **`--foreground`**: launching Zed from a terminal with `zed --foreground`
  prints the extension's own output.

## Publishing

Zed's extension registry is the
[zed-industries/extensions](https://github.com/zed-industries/extensions)
repository. Publishing means adding this directory as a git submodule there
and an entry to its `extensions.toml`; the registry builds the extension from
the pinned commit. See Zed's
[developing extensions](https://zed.dev/docs/extensions/developing-extensions)
guide for the current procedure.
