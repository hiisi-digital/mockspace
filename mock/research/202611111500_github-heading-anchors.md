---
date: 2026-11-11
scope: how GitHub turns a markdown heading into its anchor, for `refpath::forge_slug`
status: settled by the source named below
---

# GitHub heading anchors

A reference to a heading is the heading's anchor, folded, so `forge_slug` has to give what GitHub
gives. GitHub's own renderer is not published; the slugger it is documented to follow is
`github-slugger` (https://github.com/Flet/github-slugger, `index.js` and `regex.js` on `master`,
fetched 2026-11-11).

## The algorithm

The whole of `slug(value)` in `index.js`:

```js
if (!maintainCase) value = value.toLowerCase()
return value.replace(regex, '').replace(/ /g, '-')
```

`regex.js` is generated and lists every character to delete. It opens with
`[\0-\x1F!-,\.\/:-@\[-\^`\{-\xA9 ...`: the controls (so a tab), the ASCII punctuation except `-`
and `_`, and the rest of the characters that are neither letters, marks, numbers nor connector
punctuation. The space (0x20) and the hyphen are kept. A no-break space (0xA0) falls inside
`\{-\xA9` and is deleted.

So, in order: lowercase, delete, then every remaining ordinary space becomes one hyphen.

## What follows

- A run of spaces is a run of hyphens: `a  b` is `a--b`. There is no collapsing.
- Nothing is trimmed: a trailing `-` or a trailing space survives, so `Foo -` is `foo--`.
- A hyphen between spaces stays: `a - b` is `a---b`.
- A tab is deleted, not a separator: a tab between two words joins them.
- `C++ & Rust` is `c--rust`: the punctuation goes, both spaces stay.

## Not covered

The slugger keeps combining marks; `forge_slug` keeps what `char::is_alphanumeric` keeps, and the
standard's names are ASCII, so a heading with a mark folds to no name either way. Duplicate
headings get a `-1` suffix from the stateful slugger; a reference names the first.
