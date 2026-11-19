# 0004: A web panel to manage and edit the documentation

Status: accepted as a design (2026-11-19). Nothing of this is built. The four questions at the end
were settled by the maintainer on the same day.

## Context

Documents are edited today as files, and teams share them through Git. The first plan went
further and left editing to GitHub (its web editor, pull requests and history), which is also what
`PLAN/PARIDAD-GITBOOK.md` records for the rows about the editor, versions, change requests and
roles. The maintainer rejected that: most people who run Anaquel would put it on a VPS, on
Cloudflare Workers or on another host in the cloud, and they need a place to manage and edit all
the documentation of an installation, as GitBook does, without having to know Git.

Requirements stated by the maintainer, and ones that follow from the project rules:

- It runs on any Windows (including Windows Server) and any Linux distribution.
- It can be used by people who only want a browser, on a server they run themselves.
- No AI in the product and no recurring cost to run it.
- It must not turn the static site into something that needs a server: the published site stays
  plain files; only the panel is a program that runs.

## What the panel has to do

Taken from the rows of `PLAN/PARIDAD-GITBOOK.md` that were left to GitHub:

1. A list of everything in the installation (shelves, volumes, pages, status, last change).
2. An editor for one page, with a live preview rendered by the same engine as the build.
3. History of each page and of the library, with a comparison between two versions and a restore.
4. Change requests: a set of edits made apart from the published text, reviewed, approved and merged.
5. Roles: reader, editor, reviewer and administrator, with the rule of each written down.
6. Publishing: run the build and make the result the live site, and say what failed when it fails.
7. Git underneath, optional: the library folder stays an ordinary folder that Git can track, so a
   team that already uses Git loses nothing.

## Proposed decision

One more mode of the same program, `anaquel studio`, and not a separate product.

- **Source of truth.** The library folder (Markdown and `toml` files) on the server's disk. The panel
  reads and writes those files; it keeps no second copy of the text in a database. History and
  change requests are Git commits and branches kept in a repository the panel manages, so the
  folder stays readable by any other tool.
- **Accounts.** Local accounts stored on the server with a slow password hash, sessions in a
  cookie that scripts cannot read, and roles checked on every request. Single sign-on is not in the
  first version.
- **Rendering.** Preview, validation and publishing call the same crates as `build`, so what the
  editor shows is what the build writes and the same diagnostics (`AQ####`) appear next to the text.
- **Editor, first version.** Markdown source with a live preview and the directives of the engine
  inserted from a menu. A block editor like GitBook's is a later phase and is the largest piece.
- **No real-time co-editing in the first version.** Two people edit different pages, or work in
  separate change requests; one page is locked to one editor at a time.
- **Where it runs.** A native program on a VPS, a Windows or Linux server, or a container, behind a
  reverse proxy that provides TLS. The published site is served by any web host.

## What this does not decide

- **Cloudflare Workers.** A Workers site is files; the panel is a program that needs a disk and
  Git, which a Worker does not have. Running the panel itself on Workers would need another storage
  back end (an object store and a database in place of the folder) and a build to WebAssembly. That
  is a separate, large piece, and it is not promised here. A panel on a VPS can publish to a
  Workers site.
- **Single sign-on, SCIM, comments, analytics and real-time presence.** Left for later or not done.

## Phases

1. **Local single user.** `anaquel studio` binds to `127.0.0.1`: page list, editor with preview,
   validation, save to disk. No accounts. This is what most of the later phases build on.
2. **Server with accounts.** Users and roles, sessions, history and restore, publish, an audit log.
3. **Change requests.** Branches, a comparison view, review and approval, merge.
4. **Git exchange.** Push and pull with a remote, for teams that keep a copy on GitHub or GitLab.
5. **Block editor.** After the above works and has been used.

## Tests this decision will require

- Every route checks the role: a test per route and per role, with the refusal as the case that must
  pass. A path outside the library, with `..`, a link or a drive letter, must be refused in every
  route that takes one.
- Cross-site request forgery, session fixation, login rate limits, passwords never in logs.
- The save is atomic: a crash between two writes leaves the old file or the new one, never half.
- A page saved by the panel builds with the same output as the same file saved by hand.
- Run on Windows and on Linux in CI, and on at least one Windows Server and one musl Linux image.

## Costs and the best argument against

- It is the largest piece of the project so far: a web server with accounts, an editor and a
  history. Phase 1 is a sizeable piece on its own and phases 2 and 3 are the bulk; no time estimate
  has been made.
- A program that writes files and holds logins is an attack surface the static engine does not
  have. The engine today has no network service except the preview on `127.0.0.1`.
- The strongest argument against: a good editor exists already as a hosted service, and a Git
  host's own editor covers the same need for teams that know Git. The reply is the requirement
  above: people who do not use Git and want to run the editor themselves.

## Settled by the maintainer (2026-11-19)

1. Accounts are local to the server, with roles; the first server version does not rely on a
   reverse proxy for the login.
2. Phase 1 (local, single user) is released on its own, before any server mode.
3. A panel running on Cloudflare Workers is a later phase, not a goal now.
4. The panel's own text is in English and Spanish, like the theme.

## Rejected

- Leaving editing to GitHub only: does not serve people without Git or without GitHub.
- A hosted service run by the project: the project spends no money and holds no one's documents.
- Copying a third-party CMS: its setup and its backend are another product to install and keep up
  to date, and the ones checked in the first plan need a database, a Node server or a login
  service of their own.
