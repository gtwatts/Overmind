---
description: start a video job of any style (educational, commercial, social, investigative, motion graphics, whiteboard)
argument-hint: "<who it's for / kind of video / style>"
skills: [photocraft, blender, remotion-video-production, ffmpeg-skill]
files: [~/Documents/projects/ai-video-examples/index.md]
pipelines: [blender-motion-graphics, storyboard-pipeline-creator, high-end-whiteboard]
---
# Video job

Brief: $ARGUMENTS

Go to work on this brief straight away. If the brief is empty, ask for it in one short question and stop.

## 1. Load context first

Read the pipelines and files listed in the context section below before planning. They are reference material: do not edit them.

- If the brief clearly matches one of these pipelines, also read its folder (it sits next to the pipelines listed below): `explainer-video`, `education-lesson`, `commercial-spot`, `documentary-short`, `research-video`, `ai-film-preproduction`, `person-hologram-vfx`, `video-infographic-explainer-storybook-pdf`.
- If the example library index exists, skim it for reference examples matching the style (its folder also has `patterns.md` and `html-video.md`). Skip it if it's missing or empty.

## 2. Infer, don't interrogate

Infer style, length, aspect ratio, audience and deliverables from the brief. Defaults by style:

- social: 9:16, 30-60 s
- commercial: 16:9, 30 s
- educational / explainer: 16:9, 2-4 min
- investigative / documentary: 16:9, 3-8 min

Default deliverables: H.264 MP4 at 1080p/30fps plus a poster frame. State your assumptions in one line and proceed. Ask only if something truly blocks work, like brand assets or facts you can't get any other way.

## 3. Tool map (pick what fits; not every job needs every tool)

- **Blender** (blender-mcp / blender skill): 3D construction, blueprints, product shots, and high-end After-Effects-style animation.
- **Photocraft** (photocraft skill): assets like overlays, lower thirds, titles, end cards, text and logo compositing, transparent PNGs, PSD masters, frame cleanup and grading.
- **Codex image generation** (built-in / gpt-image skills): generative images, backgrounds, style frames, concept art.
- **Remotion + HTML** (Remotion skills): storyboarded composition and timeline, typography motion, data visuals, final scene assembly.
- **FFmpeg** (ffmpeg-skill): audio, narration, music beds, muxing, concatenation, encoding and final QC.

## 4. Light structure

Use the pipelines as guidance, not a rigid checklist. A typical flow: brief, then a short storyboard or shot list, then assets, then composition, then audio and encode, then review. Scale it to the job: a 15 s social clip doesn't need a booklet. Keep work in the current directory, save intermediate files, and finish with the output paths and a short summary.
