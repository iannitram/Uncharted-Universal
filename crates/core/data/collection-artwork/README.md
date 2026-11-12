# Collection artwork reference assets

These five original, game-data-free images are the fixed output of
`packaging/windows/make_uncharted_art.py` from the pre-Rust artwork revision
`5812afc708b63fa7cbc2df2d965922aff2971198`. The old generator used an unseeded
film-grain source, so these files freeze one output while preserving its composition.
The Rust `generate-art` command copies them verbatim; package builds do not need Pillow.
