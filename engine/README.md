# Revy Engine Foundation

This directory contains the source foundation of the Revy engine.

Revy began from the Bevy 0.19 codebase and now evolves independently. The
internal `bevy` and `bevy_*` crate names are retained to keep the inherited
module graph stable. New game projects should use the `revy_engine` dependency
alias. The underlying `arisna_engine` package remains available for existing
projects and should not be removed without a dedicated migration release.

```text
revy_game
    -> revy_engine (dependency alias)
        -> arisna_engine (compatibility package)
        -> internal Bevy-derived crates
```

The original Bevy 0.19 commit is preserved by the
`arisna-bevy-0.19-baseline` tag. Upstream source provenance remains available
through the `upstream` Git remote.

The inherited source remains available under the MIT or Apache-2.0 license.
See `LICENSE-MIT`, `LICENSE-APACHE`, and `CREDITS.md`.
