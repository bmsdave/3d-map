# Contributing

Contributions must preserve deterministic fixtures and the alpha release
boundary. For behavior changes, add a focused failing test first, then make the
smallest implementation and run the relevant Rust and browser checks. Rendering
changes need an intentional, visually reviewed golden update.

Do not commit real-world derived data, downloaded OSM/DEM inputs, credentials,
or large unrelated artifacts. Discuss API, MT2 format, and data-pipeline changes
in an issue before implementation.
