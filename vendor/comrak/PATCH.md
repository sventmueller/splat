# comrak 0.59.0 with one line changed

This is the crate as published on crates.io, without its repository files, with one line changed in
`src/parser/mod.rs`:

```diff
-        self.refmap.max_ref_size = self.total_size.min(100000);
+        self.refmap.max_ref_size = self.total_size.max(100000);
```

comrak stops resolving reference links once their addresses add up to the size of the document, or to 100 KB in a longer
one, so a short document loses links. cmark and cmark-gfm use 100 KB as the minimum instead. Reported upstream as
<https://github.com/kivikakk/comrak/issues/904>.

Once a comrak release carries the change, remove this folder, `[patch.crates-io]` in `Cargo.toml` and `fuzz/Cargo.toml`,
and the `exclude` entry of the workspace. `reader/assumptions.rs` fails if a comrak without it comes back, and
`reference_budget` in `crates/splat-markdown/src/doc/facts.rs` says the same limit.
