---
description: make a high-end whiteboard video (stroke reveal, organic draw-on, Blender 3D board or hybrid)
argument-hint: "<topic / audience / mode / runtime>"
skills: [high-end-whiteboard, photocraft, imagemagick, whiteboard-animator, remotion-video-production]
pipelines: [high-end-whiteboard]
---
# Whiteboard video

Brief: $ARGUMENTS

If the brief is empty, ask for it in one short question and stop.

Run this as the high-end-whiteboard pipeline. Read the pipeline files listed in the context section below before planning. They are the authoritative stage contract: follow them and don't edit them.

- Lock one mode at intake: `stroke-reveal` (default), `organic-drawon`, `blender-3d-world` or `hybrid`. Infer it when the brief makes it obvious; otherwise use `stroke-reveal` and say so.
- The drawings must explain the actual process in the brief, not generic doodles. No visible hands unless I ask for them.
- Defaults: 60-90 s, 16:9 at 1920x1080, voiceover and music on.
- Board craft: use Photocraft (the photocraft MCP tools or photocraft-cli) for layered masters, exact type, real logos, white snap and ink-to-transparent layers. Fall back to ImageMagick only for steps Photocraft can't do, and note each fallback.
- Respect every approval gate in the pipeline. Show the cost plan and wait for my explicit approval before any paid image or audio generation. Publishing or uploading needs its own approval.
- Mark unused routes N/A with a reason, as the pipeline asks.

Work in the current directory. Finish with the output paths, the chosen mode, and a short QC summary.
