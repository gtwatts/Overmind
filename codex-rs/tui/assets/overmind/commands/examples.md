---
description: find reference videos in the AI video examples library that match a brief
argument-hint: "<brief, style or technique>"
files: [~/Documents/projects/ai-video-examples/README.md, ~/Documents/projects/ai-video-examples/index.md]
---
# Find reference examples

Brief: $ARGUMENTS

If the brief is empty, ask what kind of video I'm after in one short question and stop.

Search the reference library at `~/Documents/projects/ai-video-examples` for examples that match this brief. It is read-only reference material: don't edit, rebuild or reorganize anything in it, and don't download media.

1. Read the README for the layout. Filter `index.json` by `genre`, `style`, `stack`, `tags` and `maps_to_pi_skill`, and search the cards in `examples/` and `transcripts-summaries/` for the brief's keywords (use `rg`).
2. Pick the 3-7 strongest matches. Prefer a close match in style and technique over raw view counts.
3. For each one, give the title and creator, the link, why it fits, the stack or workflow it uses, the skill it maps to, and its thumbnail path (`thumbnails/<id>.jpg`).
4. Pull the relevant playbook notes from `patterns.md`, plus `html-video.md` when a match is HTML-based, and any relevant entries from `repos.md`.
5. End with a short recommendation: which approach to borrow, and which of my commands or pipelines fits it (for example `/video` or `/whiteboard`).

Answer only. Don't start production unless I ask.
