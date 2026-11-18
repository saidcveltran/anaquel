# 0002: `relocate` moves a page and leaves a redirect

Status: accepted (2026-11-18); implemented in `anaquel relocate`

## Context

M2 ends when moving a page leaves a redirect. Two parts of that already exist in the code:
the `aliases` front matter key becomes a redirect, and the ledger of the previous build makes
a published address that leads nowhere an error (`AQ0601`). What is missing is the command that
does the move, because by hand it takes three edits that must agree.

Run on 2026-11-18 with the 0.1.0 binary, moving `guide/first-steps.md` to
`guide/primeros-pasos.md` in the demo library:

- With no alias, the build fails with two errors: `AQ0601` for `/guide/first-steps/` and
  `AQ0302` for the link in `_index.md` that still names the old file.
- With the alias and the link fixed, the build passes, `_redirects` gets
  `/guide/first-steps/ /guide/primeros-pasos/ 301`, and a small HTML page with a canonical link
  and a refresh is written at the old address.
- Moving the page back while it still lists its own address as an alias fails with `AQ0301`
  (`alias ... is already a page`) and `AQ0601` for the middle address. The author has to swap
  the alias by hand.

Read in the code, not run: every alias points straight at the live address of its page, so
chains and loops between aliases cannot form. A second move only works if the page keeps its
first alias, because the ledger also lists old redirects.

## Decision

`anaquel relocate <from> <to>` (alias `mv`) takes two page paths under `volumes/` or under
`shelves/NAME/volumes/`. It plans the
whole move, builds the catalog of the result in memory, and writes only if that build has no
error. `--dry-run` prints the plan and writes nothing.

The plan, in order:

1. Rename the file. A missing folder is created inside the volume of `<to>`; a missing volume
   or shelf is an error, because `relocate` does not write `volume.toml` or `shelf.toml`. A page
   may move to another volume, to another shelf, or from `volumes/` into a shelf; only the
   address prefix changes (`/guides/start/x/` becomes `/reference/start/x/`).
2. Add the old address to `aliases` of the moved page, keeping the aliases it already has.
   If the new address equals one of its own aliases, that alias is removed instead.
3. Rewrite every inline link and reference definition that resolves to the old file. A link
   that cannot be rewritten stops the command before anything is written.
4. Write through temporary files and renames; on a failure, restore what was already moved.

Special cases:

- A page with `slug` keeps its address when only the file name changes: no alias is added.
- A draft has no address: the file is renamed and nothing else changes.
- Moving `_index.md` moves its folder; every page below gets its own alias, listed in the plan.
- Two moves in one command that swap or chain addresses are refused; run them one at a time.

## Edge cases and the codes that answer them

| Case | Answer |
| --- | --- |
| `<from>` missing, not `.md`, or outside `volumes/` and `shelves/NAME/volumes/` | `AQ0901` |
| `<to>` has a segment the address rules refuse (`url_segment_problem`) | `AQ0902`, same reason text |
| `<to>` is under a volume or shelf that does not exist | `AQ0902`; nothing is created |
| Move between shelves, or from `volumes/` into a shelf | allowed; the old address becomes an alias of the new one, and a volume name may repeat in two shelves |
| The moved page uses a `[[term]]` or a relative link its new volume does not resolve | the final build reports `AQ0304` or `AQ0302`; nothing is written |
| `<to>` is a Windows device name (`con`, `nul`, `com1`...), with or without extension | `AQ0902` on every system, so a library moved on Linux still opens on Windows |
| `<to>` and another file are the same after case folding | `AQ0902`; a rename that only changes case goes through a temporary name |
| The destination path, made absolute and with the temporary name, is longer than 259 UTF-16 units, or a segment is longer than 255 bytes with the temporary name | `AQ0902` |
| A destination segment that starts with `.`, ends with a space or a dot, or holds `< > : " \| ? * \ # %` | `AQ0902`, for every segment and for `slug` pages and files too |
| A destination folder that differs from an existing folder only by letter case | `AQ0902`; a case-insensitive disk cannot tell them apart |
| A destination that passes through a symbolic link | `AQ0902` |
| The address changes only by letter case | `AQ0902` |
| `<to>` is already a page, or an alias owned by another page | `AQ0903`, naming the owner |
| Swap or chain inside one command | `AQ0905` |
| A link to the moved page that the rewriter cannot place | `AQ0904`, nothing written |
| The old address was already an alias of another page | `AQ0903` |
| Page deleted later | no new code: `AQ0601` already fires for the alias |

The numbers are a proposal, in a new `09xx` family; the maintainers choose them. A collision
found by the final build keeps `AQ0301`.

## Tests that would cover it

- Per case in the table, one test that checks the code and that the tree is unchanged.
- Name classes as a table, like `names.rs`, run on both operating systems.
- Move, then `build --strict`: no error, redirect in the ledger and in `_redirects`.
- Move twice, then move back: both old addresses still redirect, none points at itself.
- Interrupted write restores the original files (a failing writer injected).
- `slug` page, draft and `_index.md` moves.
- Moves between volumes, between shelves and from `volumes/` into a shelf, on a library shaped
  like `examples/multi-shelf` (its two `start` volumes share a name), and a move whose page uses
  a term of its old volume, which must leave the tree unchanged.
- File tests run in CI on Linux and Windows; macOS is not covered.

## Limits

- A single move is one page. A folder moves only through its `_index.md`, and then its static
  files move with it; their old addresses get no redirect and the ledger does not track them.
  The plan says how many files change address without a redirect.
- Raw HTML in a page is already an error (`AQ0402`), so it never reaches the rewriter. A link in
  the `href` or `image` of a `:::` block that points at a moving page, or sits in a page that
  moves, stops the move (`AQ0904`): a page with cards of relative `href` does not move between
  folders. An `aliases` list with a comment, or a file that mixes `\r\n` and `\n`, stops it too.
- The previous build's ledger is not read. Instead the result is compared in memory: every
  published address and redirect of the library before the move must still exist after it,
  or nothing is written.
- A crash in the middle of a write (power loss, a killed process) is not undone: there is no
  journal. A failed write is undone; a crash can leave files named `.NAME.anaquel-tmp` next to
  the old and the new places.
- The Windows rules (reserved names, forbidden characters, the 259-unit path) and the
  case-insensitive folder rule are checked on every system and tested on Linux only; they were not
  run on Windows or macOS.
- That terms and links resolve per volume is read from the error texts (`no term ... in this
  volume`) and from `examples/multi-shelf`; moves between volumes and shelves are tested.
