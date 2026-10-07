---
description: create or edit image assets with Photocraft (lower thirds, titles, thumbnails, overlays, PSD masters, batch frames)
argument-hint: "<asset to make or file to edit>"
skills: [photocraft]
---
# Photocraft asset job

Request: $ARGUMENTS

If the request is empty, ask what to make or edit in one short question and stop.

Use Photocraft for this:

- Prefer the photocraft MCP tools for multi-step, interactive edits; use `photocraft-cli` for one-shot or batch jobs. MCP file access is limited to `$$HOME` and `/tmp`.
- Discover before guessing: list the commands and read their parameter schemas before calling them.
- Keep an editable master (`.psd` or `.pcraft`) next to every flattened export (PNG, WebP or JPG), unless I only asked for a flat file.
- Never overwrite an input file, and never write a batch's output into its input folder. Write new files beside the originals.
- Use only the real logos and brand files I've given you; never invent a mark.
- If Photocraft can't do a step, say so and use ImageMagick for that step only.

Verify before reporting: reopen each output (`photocraft-cli info`, or view the image) and check its size, alpha and bounds. Finish with the output paths and what each one is for.
