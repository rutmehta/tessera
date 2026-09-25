# Agentic Editing — The Agent Makes the Base Edit, the App Is Where You Fine-Tune

## 1. Concept
A shoot comes in; an agent culls, groups, and produces a complete *base edit* for every keeper using **our engine's own tools** (recipe parameters, masks, retouch operations), not image generation. The photographer opens the app to review, tweak, and add creative intent. Every agent action is an ordinary recipe step: inspectable, undoable, and editable by hand.

## 2. Architecture
```
Photographer intent ──► Agent (LLM planner) ──► Engine Tool API ──► Recipe ──► Render
                          ▲                        │
                          └── Critic (VLM + metrics) ◄── rendered preview
```
- **Engine Tool API**: every operator, mask type, retouch action, and library action is exposed as a typed tool (also published as an MCP server so external agents and scripts can drive the app headlessly). Examples: `set_tone(exposure, contrast, highlights, …)`, `create_mask(kind="subject"|"sky"|"person:id"|"depth", ops)`, `adjust_mask(mask_id, params)`, `remove_object(mask_id)`, `retouch_skin(person_id, strength)`, `apply_style(profile_id, amount)`, `crop(rect, angle)`, `compare(image_a, image_b)`, `get_histogram()`, `get_scores(image)`.
- **Perception inputs** to the planner: scene/object labels, faces and identities, quality scores, depth, histogram/clipping stats, EXIF, the user's style profile, and a low-res render. The agent never sees or edits pixels directly.
- **Critic loop**: after each plan step the engine renders a preview; a VLM plus objective metrics (clipping, skin-tone ΔE against target, noise, contrast distribution) evaluate against the intent; the planner iterates a bounded number of times.
- **Style profile**: a per-user model trained on the user's historical recipes (image features → slider/mask decisions), used both as a direct predictor for the base edit and as a prior the LLM planner conditions on. Cold start via a questionnaire and a few reference edits.
- **Batch mode**: runs across a whole shoot with consistency constraints (same person → same skin treatment; same scene group → same WB/tone; sequences share a look). Produces a review queue sorted by the critic's confidence so the photographer looks first at what the agent was least sure about.
- **Fine-tune surface**: the Develop UI shows the agent's steps as a named history group ("Agent base edit") with a single "amount" slider to fade the whole group, per-step toggles, and normal manual editing on top. Accepting/changing steps feeds back into the style profile.
- **Explainability**: each step carries a one-line rationale ("lifted shadows +25 because faces measured 1.3 EV under mid-grey"); the user can ask the agent to redo a step with a natural-language instruction ("warmer, keep the sky").
- **Safety/scope**: no generative pixels in base edits; retouch limited to configured guardrails (e.g. no body reshaping unless enabled); all steps reversible; provenance recorded (Content Credentials list "AI-assisted, non-generative edits").

## 3. Phasing
1. **Style-profile auto-edit** (non-LLM): predictor outputs base recipe; batch apply; review queue. Fastest to ship, largest time saving.
2. **Tool API + MCP server**: expose engine to scripts/agents; ship an in-app scripting console.
3. **Planner + critic**: LLM (local or cloud, user choice) plans multi-step edits incl. masks and retouch; natural-language redo per image or per shoot.
4. **Conversational fine-tuning** in the UI and cross-image consistency reasoning.

## 4. Success metrics
- Time from import to client-ready gallery for a 2,000-image wedding: target < 1 hour of human time.
- Percentage of agent base edits accepted without change: target > 70% after 3 shoots of feedback.
- Zero pixel-generative content in base edits; 100% of steps reproducible from recipe.
