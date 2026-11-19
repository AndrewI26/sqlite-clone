# Plan

A from-scratch SQLite clone in Rust. Each layer only talks to the one below it.

```
SQL text
  -> REPL        reads lines, handles .exit / .tables etc.
  -> Tokenizer   &str -> tokens
  -> Parser      tokens -> AST (CREATE / INSERT / SELECT ...)
  -> Executor    walks the AST, evaluates WHERE, uses the catalog
  -> B+tree      rowid -> row bytes, one tree per table
  -> Pager       4 KiB pages, cached in memory, the only file I/O
  -> .db file
```

## How the pieces work

- **Pager**: the file is an array of 4096-byte pages; page `n` is at offset `n * 4096`. Load pages on demand, mark changed ones dirty, write them back on exit.
- **Pages / nodes**: leaf pages hold rows sorted by rowid (slotted page: pointer array at the front, cells packed from the back). Interior pages hold `(child page, key)` pairs.
- **Records**: a row is encoded as column count, a type tag per column, then the values.
- **B+tree**: insert finds the right leaf; when it's full, split it in half and push a key up to the parent (the root splitting adds a level). Leaves link to their right sibling so scans are easy.
- **Catalog**: page 0 is a special table of `(name, root_page, sql)`, like SQLite's `sqlite_schema`. On open, read it to learn which tables exist.
- **Parser**: hand-written recursive descent; precedence climbing for expressions.
- **Executor**: SELECT = scan the table's tree, decode each row, filter, project.

## Milestones

1. REPL loop with `.exit`
2. Hardcoded table: `insert 1 alice` / `select`, rows saved to a single page, still there after a restart
3. Tokenizer + parser for `CREATE TABLE`, `INSERT`, `SELECT *`
4. Leaf pages + record encoding, so real tables work
5. B+tree splits (more than one page of rows)
6. Catalog on page 0: multiple tables, `.tables`
7. `WHERE`, column lists, `ORDER BY`, `LIMIT`
8. `UPDATE` / `DELETE`
9. Stretch: transactions/journal, indexes, joins
