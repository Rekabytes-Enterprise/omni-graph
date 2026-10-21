# Memory image attachments

## Problem and usage

UI memories need durable visual references in addition to text. Source-path-only
references break when a screenshot moves and cannot travel safely with a shared
memory. `remember --file` now accepts `images: [{file, caption}]` alongside optional
links. Existing memories use `image add/list/export/remove`. `show`, search, and
recall expose metadata so agents know an image exists and can export it for viewing.

## Storage shape

`ImageInput` reads a bounded regular file and identifies PNG/JPEG/WebP by signature.
It prepares image bytes, a generated ID, caption, filename, size, MIME type, and
SHA-256. The source path is not stored. `Workspace::create_with_images` prepares
inputs before calling the existing graph transaction. `Store::change_with_images`
saves memory/edges/FTS, then attachments, and commits once. SQL or validation
failure rolls back everything. Each attachment belongs to one memory by foreign key.

SQLite schema 2 adds `memory_images` with a BLOB and JSON metadata. Opening a schema
1 store migrates it in a transaction after checking the application ID. Older
executables reject schema 2. Images are not loaded into graph snapshots or FTS.
Existing-memory add/remove updates the owning memory timestamp for UI refresh.

## UI and access

The selected memory loads metadata at `/api/images/MEMORY_ID`; bytes load at
`/api/images/MEMORY_ID/IMAGE_ID`. Both routes pass through existing Host/Origin,
method, and remote authentication boundaries. Ownership is checked through the
accessible memory and exact memory/image pair, with no filesystem path route.
Raster MIME types, nosniff, CSP, and text-only captions prevent SVG/HTML injection.
The selected-memory request generation prevents a stale image response appearing
under a different selection. TUI users export images or use the browser UI.

## Sharing

Only images owned by explicitly shared project/task memories enter shared bundles.
Bundle schema 2 carries base64 bytes, metadata and checksum; schema 1 text bundles
remain accepted. Import verifies image identity, ownership, signature, size and
checksum and inserts images in the same transaction as imported memories/edges.
Conflicting IDs abort rather than overwrite. Identical import is idempotent.

## Design comparison and limits

Two shapes were considered sequentially: filesystem assets referenced by memory,
or BLOBs owned by the same SQLite store. Filesystem assets expose copying, cleanup,
path traversal, and rollback coordination to multiple writers. BLOBs reuse local
atomicity, ownership, backups, and sharing, so they are the chosen shape.

We accept larger SQLite databases in exchange for durable references and one
transaction. Limits are eight images per memory and 8 MiB per image. No resizing,
metadata stripping, OCR, visual embeddings, or approval inference is performed.
File signatures identify types; they do not fully decode or validate every pixel.
Base64 is used only for portable exports ([base64 0.22 API](https://docs.rs/crate/base64/0.22.1/source/src/lib.rs));
the database stores raw bytes. Image edits and semantic image search are separate
features.
