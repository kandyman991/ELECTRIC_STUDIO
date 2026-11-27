# The components library

Parts shared by every project, from every manufacturer. A project still keeps its own copy of
each part it uses (`parts.json`), so it opens without the library; the library is where those
copies come from and how they are kept up to date.

## On disk

```text
components/
  library.json              {"format": 1, "name": "Workshop"}
  parts/<manufacturer>.json that maker's parts, sorted by order number
  parts/no-manufacturer.json parts without one
```

Pretty-printed JSON, one file per manufacturer, so a change to one maker's parts changes one
file and the folder diffs cleanly in git. A part is known by manufacturer and order number
together, both compared without regard to case, so two makers can use the same number. Saving
is the same all-or-nothing step as a project's, with the same lock, and is refused if someone
else saved the folder since it was read. A folder is never both a project and a library.

## A part

Order number, manufacturer, description, company code, contact blocks (`13-14 21-22`), size in
mm, product class (an ETIM or ECLASS class, `EC000066`), technical data and a data sheet. The
technical data is a list of named lines in the manufacturer's order, each value text with its
unit (`Rated current AC-3` = `9 A`), at most 500 lines, names unique.

## Importing a list

`ecad library import <library> <list>` reads CSV (comma, semicolon or tab; UTF-8, UTF-16 or
Windows-1252) and Excel (`.xlsx`, `.xlsm`, `.xls`, `.ods`).

- **Columns** are found by their titles in English, Italian, German or French: order number
  (`Order number`, `Codice articolo`, `Bestellnummer`, `Référence`...), manufacturer,
  description, code, contacts, width, height, depth or one size column, class, data sheet. A
  list with only a `Codice` column (an Italian price list) uses it as the order number. Lists
  with titles of their own name them: `--map number="Cod. Art."`.
- **Every other column** becomes technical data under its own title, unless `--data` picks
  some or `--no-data` drops them.
- **The header** needn't be the first row: the first 30 rows are searched (or `--header-row`).
  In a workbook, the first sheet with a header is read (or `--sheet`).
- **The manufacturer** of rows that don't name one: `--manufacturer` (a maker's own price list
  rarely has the column).
- **Rows** without an order number, with a size or contact block that isn't one, or repeating
  a part are left out and listed; the rest import. Group headings (a row with text but no
  number) are listed too.
- **Parts already there** get every field the list has; the fields it leaves empty keep their
  values, and its technical data replaces lines of the same name and adds the rest. A price
  list doesn't wipe technical data another list brought.

`--dry-run` shows what would change. Lists are read up to 64 MB and 200 000 rows.

## Into a project

- `ecad library add <project> <library> <number>... [--manufacturer]` copies parts into the
  project's catalogue; a number the library has from more than one maker needs
  `--manufacturer`.
- `ecad library update <project> <library>` brings every part of the catalogue up to date, and
  lists the ones the library doesn't have.
- `ecad import-parts <project> <list>` puts a list straight into a project, with the same
  reading options; the app's *Import list...* in the parts catalogue does the same.

In both directions newer data fills in and replaces, and never wipes what was entered only in
the project, such as contact blocks. A project's part from one maker is never replaced by the
same number from another.

## Coming next

- M7b: the library in the app (search, add to the project, update) and over MCP.
- M7c: BMEcat with ETIM features, the free XML catalogues many manufacturers publish.
- M7d: a default symbol per product class, so an imported part can be placed directly.
- Then ECLASS Advanced (after checking its licence terms) and AASX.
