# Commit style

Format: `type{separator}message`

The commit gate enforces this, so a subject that breaks it is refused at commit and again at push. The rules and the types below are generated from the gate's own configuration, `[lints.commit-subject]` in `mockspace.toml`.

## Rules

{commit_rules}

## Types

{commit_types}

{commit_breaking}

A type outside this set is refused. A project adds one with `extra_types = "name"` under `[lints.commit-subject]` in `mockspace.toml`, or replaces the set with `types`. Setting `commit = "off"` there turns the check off.

## Examples

```
{commit_examples}
```

Authorship trailers are the byline policy's, set in `{mock_dir}/agent/config.toml` under `[attribution]`, and are checked separately.
