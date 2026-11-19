# 0006: `anaquel studio`, phase 1 (local, single user)

Status: proposed (2026-11-19). Nothing of this is built. It narrows [0004](0004-web-panel.md), which
accepted the panel and settled its four questions; this page decides how phase 1 is built and cut
into pieces that can each be merged and tested alone.

## Scope of phase 1

From 0004: `anaquel studio` binds to `127.0.0.1`; page list; an editor with a live preview;
validation; save to disk. No accounts, no history, no change requests, no publishing, no Git.
The panel's own text is in English and Spanish.

Out of phase 1, on purpose: anything that needs a second user (roles, locks held by another
person, change requests) and anything that leaves the machine.

## What was read in the code (not run)

- `anaquel-preview` already serves the built site with axum and tokio on `127.0.0.1` only and
  refuses a request whose `Host` is not `localhost`, `127.0.0.1` or `[::1]`, against DNS
  rebinding. `studio` reuses that rule.
- `anaquel-pipeline` has `ingest` (reads the folder into an in-memory `Ingested`) and
  `analyse_sources`, which takes that value and returns the catalog, the composed pages and the
  report. `relocate` already builds the result of a change in memory and compares it before
  writing. A preview of unsaved text is therefore the same call with one page's text replaced.
- No function today renders one page to HTML for display from an in-memory library; the preview
  server serves the files `build` wrote. That function is new work (piece 3).

## Decisions

1. **A new crate `anaquel-studio`, and a thin `studio` subcommand in `anaquel`.** The crate holds
   the logic and the HTTP layer; the subcommand parses arguments and calls it. Same split as
   `anaquel-upgrade`.
2. **The folder stays the only store.** Reads and writes go to the library's Markdown and `toml`
   files. The panel keeps no copy in a database and no state file beside the library.
3. **Paths are a type, not a string.** A `LibraryPath` can only be built from a relative path of
   normal segments that resolves, symbolic links included, inside the library root; `..`, a
   leading `/`, a drive letter, a `\` segment, a reserved Windows name and a path that crosses a
   link are refused at construction. No route handler handles a raw string path.
4. **Saving is atomic and conflict-aware.** A save names the hash of the text the editor loaded.
   The server writes the new text to a temporary file in the same folder (`create_new`), copies
   the permission bits, flushes, and renames it over the page; if the hash of the file on disk no
   longer matches, nothing is written and the editor is told to reload. A crash leaves the old
   file or the new one. The write path is the one `relocate` already proved; it moves to a shared
   module rather than being copied.
5. **The preview is the build.** The editor sends its unsaved text; the server replaces that page
   in the ingested library, runs the same analysis as `build`, and returns the page's HTML and the
   diagnostics (`AQ####`) for that page. A page saved by the panel must build with the same
   output as the same file saved by hand: a test builds both and compares.
6. **A write needs more than a loopback address.** Any web page the user visits can make their
   browser send a request to `127.0.0.1`. Every route that reads or writes checks `Host`
   (rebinding) and, for anything that is not a plain read, `Origin` (cross-site requests) and a
   random token printed once at start in the URL the program opens and kept in a cookie that
   scripts cannot read. A request without them is refused with a clear message. This is the part
   most likely to be forgotten, so it comes with its own tests.
7. **The interface is plain files embedded in the program.** HTML, CSS and one script, no npm and no
   framework, served under the same strict Content-Security-Policy as the site (no inline script
   or style). The editor is a `<textarea>` with a preview pane in the first version; the
   directive menu inserts the text of a directive. A richer editor is phase 5 of 0004.
8. **Two languages from the start.** Interface texts live in one table per language (`en`, `es`),
   chosen by the browser's `Accept-Language` and switchable in the page; a test fails if a key
   exists in one language and not in the other.
9. **No page lock in phase 1.** One user, one process. If a second `studio` is started on the
   same folder, the conflict check of decision 4 is what protects the files.

## Pieces, each merged alone and each with its tests

1. `anaquel-studio` core: `LibraryPath`, the page list (shelves, volumes, pages, status, last
   change), the shared atomic write with the hash check. No HTTP.
2. A pipeline function that analyses the library with one page's text replaced and returns that
   page's HTML and diagnostics.
3. The HTTP layer: routes, the request guards of decision 6, the embedded interface, the texts in
   two languages.
4. The `anaquel studio [ROOT] [--port N]` subcommand, `doctor` mention, README rows, CHANGELOG.
5. Release of phase 1 on its own (0004, settled point 2).

## Tests this phase requires

- `LibraryPath`: every refused form above, each as its own case, including a link inside the
  library that points outside it, and a case that must be accepted for each.
- Save: the old file or the new one after an injected failure at every step (the loop used for
  `relocate`), a stale hash refused with the file untouched, permission bits kept on Unix.
- Guards: a request with a foreign `Host`, a foreign or missing `Origin` on a write, a missing or
  wrong token, and a token in the query string of a later request, each refused; the same
  requests with the right values accepted.
- Preview: output equal to the build's for the same text, diagnostics of the page shown, a page
  with an error still returns its diagnostics and no HTML that pretends to be the site.
- Text tables: same keys in both languages.
- Run on Windows and Linux in CI; the HTTP tests bind port 0 and use no fixed port.

## Costs and the best argument against

- Phase 1 alone is a few thousand lines and a new attack surface: a program that writes files
  and listens, even on loopback. Decision 6 is the answer, and it has to be tested, not assumed.
- The strongest argument against: a text editor and `anaquel serve` already cover one person
  on one machine. Phase 1 is justified only as the base of phases 2 and 3, and it is released
  alone so that its use shows whether the editor is worth building on.
- Not known and not measured: how large the library can be before an analysis per keystroke is
  too slow. The preview should run on a pause in typing, and piece 2 includes a measurement on
  the 271-page example library; if it is too slow, the design changes (analyse once, recompose
  the one page) before the interface is written.

## Rejected

- A separate program for the panel: two binaries to install and keep in step, for no gain.
- A database for the page text: a second source of truth that can disagree with the files.
- Opening the editor on a fixed port without a token: any page in the user's browser could write
  to the library.
