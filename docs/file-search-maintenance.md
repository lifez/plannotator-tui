# Maintaining the File Search branch

The local feature lives on `feature/file-search`. Keep File Search code, tests and
user documentation in a dedicated commit; do not mix unrelated changes into it.
No plugin framework or additional dependencies are needed.

- Fork remote: `origin` (`lifez/plannotator-tui`).
- Upstream remote: `upstream` (`plannotator/plannotator-tui`).
- Initial feature base: `f35f70ebd392198f0ab0299ecbdc9c834184c899`.

## Finish the initial commit

Set your actual commit email locally if Git has none, then commit the staged files:

```sh
git config --local user.email 'YOUR_EMAIL'
git commit -m "feat(search): quick open markdown files by filename"
```

## Bring in upstream updates

Start with a clean working tree (commit or stash pending tracked and untracked
work first). For an unpublished feature branch:

```sh
git fetch upstream
git switch feature/file-search
git rebase upstream/main
```

If Git reports conflicts, resolve them, stage the resolved files and run
`git rebase --continue`. Use `git rebase --abort` to return to the previous state.
Do not blindly choose our entire version of a file: preserve upstream changes.

If the branch is already shared, prefer `git merge upstream/main` instead of
rebasing so collaborators do not have to recover from rewritten history.

After either operation:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets
```

The likely conflict points are `app/mod.rs`, `app/input.rs`, `app/draw.rs`,
`cli.rs`, `main.rs` and `tree.rs`. The matching/scanning modules and popup/tests
are isolated in `file_search.rs`, `file_search/`, `app/file_search.rs` and
`app/file_search/` under `crates/plannotator-tui/src/`.

## Export and apply a patch

After committing and rebasing onto upstream, export the feature commits outside
the repository (do not commit generated patches):

```sh
git format-patch --output-directory ../file-search-patches upstream/main..feature/file-search
```

On a clean checkout of the target upstream version:

```sh
git switch -c feature/file-search
git am --3way ../file-search-patches/*.patch
```

Resolve conflicts and use `git am --continue`, or cancel with `git am --abort`.
Run the same checks after applying. Export from a rebased branch; after merges,
this range may also include unrelated fork commits, so inspect it before export.

Before the initial commit exists, a staged diff can be backed up instead:

```sh
git diff --cached --binary > ../file-search-working-tree.patch
```

Apply that diff with `git apply --3way ../file-search-working-tree.patch` on a
clean target checkout. Unlike `format-patch`, this does not carry commit metadata.
