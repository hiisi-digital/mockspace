# `near-copies` over kaski at `origin/dev`

The `near-copies` tool of `mockspace-extra-lints` run over every kaski crate that has a `src/`
directory (149 crates under `mock/crates` and `mock/tools`), every crate compared with every other,
`--min 60`, no cap on the lists. The full output is `report.txt` beside this file.

Counts: 2614 types read, 45 near-copies, 585 same-named pairs of different shape. Of the 585, 536
line up in nothing (the unrelated pairs a name collision produces, the `Table` case), and 49 line up in
at least one member.

holds for: kaski at `origin/dev` on 3 November 2026, every crate with a `src/`, `--pub-only` off,
`--min 60`; test code and files outside `src/` are not read.

What the top of the near-copy list shows, for reading the report rather than as findings:

- `Extent` (kaski-frame-api) and `Shape` (kaski-texel-pages) are both `{ width, height }`.
- `FigureTier` (kaski-figure) and `Detail` (kaski-motion) are both `Full, Coarse, Far`.
- `Pose` (kaski-minds) and `ModelPosture` (kaski-view-contracts) are both `Standing, Sitting, Lying`.
- `FigureVertex` (kaski-figure) and `GpuFigureVertex` (kaski-render) share all four members.

Several of the 100% entries are two-member records (`id, name`; `by, tool`) that are not one concept,
which is what the two-member floor and the percentage can not tell apart, and why the list is a ranking
a person reads and no gate. Whether each pair above is one concept is a call for the owners of the two
crates and is not made here.

Run again with `cargo mock near-copies <crate>...` once the pack carries the tool.

Regenerate, from a checkout of kaski at `origin/dev` with the pack carrying the tool:

    cargo mock near-copies --limit 100000 $(for d in mock/crates/* mock/tools/*; do [ -d $d/src ] && basename $d; done | sort -u)

The committed `report.txt` was produced the same way through a driver that calls the tool directly,
before the pack reached kaski.
