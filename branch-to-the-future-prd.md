# Product Requirements Document: Branch to the Future

## 1. Product summary

**Branch to the Future** is a terminal-native developer tool that simulates the impact of a proposed code change before implementation. A developer describes a feature, refactor, or bug fix in natural language. The tool scans the repository, animates the exploration of affected files in a Ratatui interface, identifies the blast radius, compares multiple implementation paths, and generates developer artifacts such as patch plans, test plans, and visual architecture cards.

The MVP focuses on a cinematic but useful terminal experience: the codebase is explored through animated motion, affected files are revealed progressively, and each file is backed by reasoning explaining why it may need to change.

## 2. Problem statement

Developers often begin implementing changes before understanding their full impact. This leads to missed files, broken tests, hidden migration requirements, and architecture drift. Current AI coding tools tend to jump directly into code generation, but the risky part is often not writing code; it is understanding what the change will touch.

Branch to the Future helps developers answer:

- Which files are likely affected by this change?
- Which parts of the system are risky?
- What implementation paths are available?
- What tests need to be updated or added?
- What does the system look like before and after this change?

## 3. Target users

### Primary users

- Software engineers working in unfamiliar or messy codebases
- Hackathon builders trying to ship quickly without breaking everything
- Interns or new joiners onboarding into an existing project
- Solo developers maintaining fast-moving projects

### Secondary users

- Tech leads reviewing change plans
- Open-source contributors preparing pull requests
- Developer advocates creating technical walkthroughs

## 4. Product positioning

Branch to the Future is not a code generator first. It is a **change impact simulator**.

The core positioning:

> A cinematic TUI that shows developers the possible futures of a code change before they write it.

Alternative tagline:

> See the blast radius before you ship the blast.

## 5. Goals

### MVP goals

1. Accept a natural-language change request.
2. Scan a local repository.
3. Build a lightweight repo model from files, directories, imports, routes, config files, and keywords.
4. Use GPT-5.5 to infer affected files, risks, implementation paths, and reasoning.
5. Display an animated exploration of the repo in Ratatui.
6. Reveal affected files progressively with motion and status changes.
7. Show a final blast radius report.
8. Compare three implementation futures:
   - Fast Prototype
   - Minimal Patch
   - Proper Architecture
9. Generate a test plan and patch skeleton.
10. Optionally generate a visual before/after architecture card using GPT Image 2.

### Non-goals for MVP

- Full semantic code understanding across all languages
- Fully automatic patch application
- GitHub OAuth or cloud syncing
- Perfect dependency graph construction
- Production-grade static analysis
- Multi-user collaboration
- IDE plugin integration

## 6. Hackathon judging angle

Branch to the Future should demonstrate:

- Strong GPT-5.5 usage for codebase reasoning and planning
- Strong GPT Image 2 usage for developer-focused visual artifacts
- A polished Ratatui interface with TachyonFX animations
- A developer-specific productivity use case
- A demo that is visually memorable and technically credible

The winning demo should make judges feel:

> This is not another AI chat wrapper. This is a new way to interact with a codebase.

## 7. Core user flow

### Step 1: Launch app

Command:

```bash
branch-to-the-future ./sample-repo
```

The app opens a full-screen Ratatui interface.

### Step 2: Enter change request

User enters:

```text
add async resume parsing with S3 upload and status polling
```

### Step 3: Repo scan animation

The app scans the repo and animates:

- file tree materializing
- directories expanding
- detected frameworks appearing
- key files being indexed
- route/config/schema files being highlighted

### Step 4: Impact exploration animation

The app shows an animated traversal path:

```text
components/UploadForm.tsx
  ↓
api/upload/route.ts
  ↓
lib/s3.ts
  ↓
workers/parser.ts
  ↓
db/schema.sql
```

Affected files are revealed one at a time. Each file has:

- impact score
- confidence
- risk level
- reason
- proposed change summary

### Step 5: Blast radius report

The app summarizes:

- affected files
- files to inspect
- tests to add/update
- DB/API/config impact
- risks
- implementation complexity

### Step 6: Branch to the Future comparison

The app compares three implementation paths:

1. Fast Prototype
2. Minimal Patch
3. Proper Architecture

Each path includes:

- affected files
- estimated complexity
- risk level
- tradeoffs
- recommended use case

### Step 7: Artifact generation

User can press:

- `G` to generate visual blueprint with GPT Image 2
- `P` to generate patch skeleton
- `T` to generate test plan
- `R` to replay impact trace
- `E` to export report as Markdown

## 8. MVP feature requirements

## 8.1 Repository scanner

### Description

The scanner collects enough repository context for GPT-5.5 to reason about change impact.

### Inputs

- Local repo path
- Optional ignore patterns
- Optional max file size
- Optional language/framework hints

### Required behavior

The scanner should collect:

- directory tree
- package files
- route-like files
- config files
- database/schema files
- test files
- CI files
- environment/config usage
- import statements, where feasible
- symbol-like function/class names, via simple regex or tree-sitter if available

### MVP supported languages/frameworks

Prioritize JavaScript/TypeScript projects:

- Next.js
- React
- Node/Express
- NestJS
- generic TypeScript projects

Optional stretch:

- Python/FastAPI/Django
- Rust

### Suggested heuristics

Detect important files using names and paths:

- `package.json`, `pnpm-lock.yaml`, `yarn.lock`
- `next.config.*`, `vite.config.*`, `tsconfig.json`
- `src/app/**/route.ts`
- `pages/api/**`
- `api/**`
- `routes/**`
- `controllers/**`
- `services/**`
- `lib/**`
- `db/**`, `schema.sql`, `prisma/schema.prisma`
- `workers/**`, `jobs/**`, `queue/**`
- `tests/**`, `__tests__/**`, `*.test.ts`, `*.spec.ts`
- `.github/workflows/**`
- `.env.example`

### Output structure

```json
{
  "repo_name": "sample-repo",
  "root_path": "./sample-repo",
  "frameworks": ["Next.js", "TypeScript"],
  "files": [
    {
      "path": "components/UploadForm.tsx",
      "kind": "ui_component",
      "size": 4821,
      "symbols": ["UploadForm", "handleSubmit"],
      "imports": ["lib/api", "react"],
      "snippets": ["const handleSubmit = async ..."]
    }
  ],
  "routes": [
    {
      "path": "api/upload/route.ts",
      "method": "POST",
      "route": "/api/upload"
    }
  ],
  "tests": ["tests/upload.test.ts"],
  "config_files": ["package.json", "tsconfig.json"],
  "risk_signals": [
    {
      "path": "api/upload/route.ts",
      "signal": "file upload without visible validation"
    }
  ]
}
```

## 8.2 Change request parser

### Description

The app accepts a natural-language change and extracts structured intent.

### Example input

```text
add async resume parsing with S3 upload and status polling
```

### Example output

```json
{
  "change_type": "feature",
  "domain": "resume upload",
  "capabilities": ["S3 upload", "async parsing", "status polling"],
  "likely_layers": ["frontend", "api", "storage", "worker", "database", "tests"],
  "keywords": ["resume", "upload", "s3", "parser", "status", "polling"]
}
```

## 8.3 GPT-5.5 impact reasoning

### Description

GPT-5.5 receives the repo model and change request, then predicts the impact.

### Required output

```json
{
  "summary": "This change converts synchronous resume upload into an async processing flow.",
  "impact_path": [
    {
      "path": "components/UploadForm.tsx",
      "reason": "The UI must show upload progress and poll job status.",
      "impact_score": 89,
      "confidence": 91,
      "risk": "medium",
      "change_needed": "Return and store job_id, add polling state."
    },
    {
      "path": "api/upload/route.ts",
      "reason": "Current endpoint handles upload synchronously and must enqueue parsing work.",
      "impact_score": 95,
      "confidence": 94,
      "risk": "high",
      "change_needed": "Upload file, create job record, enqueue parser task."
    }
  ],
  "risk_summary": [
    "Large file timeout risk",
    "PII stored in S3",
    "Polling can become stale without timeout states"
  ],
  "tests_to_add": [
    "upload returns job_id",
    "status endpoint returns pending/completed/failed",
    "parser failure updates job status"
  ],
  "futures": [
    {
      "name": "Fast Prototype",
      "description": "Add minimal job status using local DB table and polling.",
      "complexity": "low",
      "risk": "high",
      "affected_files": ["components/UploadForm.tsx", "api/upload/route.ts", "db/schema.sql"]
    },
    {
      "name": "Minimal Patch",
      "description": "Keep most current upload flow but add job_id and lightweight parser worker.",
      "complexity": "medium",
      "risk": "medium",
      "affected_files": ["components/UploadForm.tsx", "api/upload/route.ts", "workers/parser.ts", "db/schema.sql"]
    },
    {
      "name": "Proper Architecture",
      "description": "Introduce queue, parser worker, status endpoint, failure states, and tests.",
      "complexity": "high",
      "risk": "low",
      "affected_files": ["components/UploadForm.tsx", "api/upload/route.ts", "api/upload/status/route.ts", "workers/parser.ts", "lib/s3.ts", "db/schema.sql", "tests/upload.test.ts"]
    }
  ],
  "recommended_future": "Proper Architecture"
}
```

## 8.4 Animated Impact Explorer

### Description

The Impact Explorer is the central visual experience. It shows the codebase being explored in motion.

### Required UI regions

```text
┌─ Branch to the Future: Impact Explorer ─────────────────────────────────┐
│ Change: add async resume parsing with status polling              │
├────────────────────── Repo Tree ───────────────┬──── Impact Path ─┤
│ frontend/                                      │ 1 UploadForm.tsx │
│   components/                                  │ 2 upload route   │
│   > UploadForm.tsx         ████████            │ 3 s3 client      │
│ api/                                           │ 4 parser worker  │
│   > upload/route.ts        ███████████         │ 5 schema.sql     │
│ lib/                                           │                  │
│   > s3.ts                  ███████             │ Risk: MEDIUM     │
│ workers/                                       │ Files: 5         │
│   > parser.ts              ████████████        │ Tests: 3         │
│ db/                                            │                  │
│   > schema.sql             ██████              │ [Enter] inspect  │
├───────────────────────────────────────────────┴───────────────────┤
│ Sweep status: tracing upload flow...                               │
└────────────────────────────────────────────────────────────────────┘
```

### Animation states

1. `BootReveal`
   - App title evolves from symbols into readable text.
   - Panels slide in.

2. `RepoMaterialize`
   - File tree coalesces into view.
   - Important files are revealed first.

3. `ScanningSweep`
   - A left-to-right or diagonal sweep passes over the tree.
   - Status text updates as the app indexes files.

4. `ImpactTrace`
   - Files in the predicted impact path light up one at a time.
   - The right panel receives a new path step for each discovered file.

5. `RiskBloom`
   - Risky files pulse with warning color.
   - Risk summary appears.

6. `LockIn`
   - Candidate files settle into confirmed state.
   - Final blast radius appears.

7. `ReplayTrace`
   - User can replay the impact trace animation.

### TachyonFX effect mapping

Use the following TachyonFX concepts:

- `evolve()` for title and status text boot effects
- `coalesce()` for file tree materialization
- `slide_in()` for panels
- `sweep_in()` with `SweepPattern` for scanning motion
- `fade_to_fg()` / `paint_fg()` for impacted file highlighting
- `hsl_shift_fg()` with `repeat()` or `ping_pong()` for risk glow
- `expand()` for dependency cone reveal
- `dissolve()` for screen transitions
- `sequence()` for staged animation
- `parallel()` for simultaneous glow and motion
- `CellFilter` to target only text cells, selected rows, or inner panel areas

## 8.5 File inspection screen

### Description

When a user selects an affected file, show why it matters.

### Layout

```text
┌─ File Impact ──────────────────────────────────────────┐
│ File: api/upload/route.ts                              │
│ Impact: 95/100     Confidence: 94%     Risk: HIGH      │
├────────────────────────────────────────────────────────┤
│ Why affected                                           │
│ - Current upload entrypoint                            │
│ - Must return job_id instead of final parse result     │
│ - Needs queue handoff and failure states               │
├────────────────────────────────────────────────────────┤
│ Suggested changes                                      │
│ 1. Validate file metadata                              │
│ 2. Upload to S3                                        │
│ 3. Create upload_jobs record                           │
│ 4. Enqueue parser worker                               │
│ 5. Return job_id                                       │
└────────────────────────────────────────────────────────┘
```

## 8.6 Futures comparison

### Description

Compare multiple possible implementations.

### Layout

```text
┌─ Branch to the Future ───────────────────────────────────────┐
│ > A. Fast Prototype       Complexity: Low    Risk: High│
│   B. Minimal Patch        Complexity: Med    Risk: Med │
│   C. Proper Architecture  Complexity: High   Risk: Low │
├────────────────────────────────────────────────────────┤
│ Selected: Proper Architecture                          │
│ Adds queue, parser worker, status endpoint, tests, and │
│ failure states. Best long-term path.                   │
└────────────────────────────────────────────────────────┘
```

### Required fields per future

- name
- summary
- complexity
- risk
- affected files
- benefits
- drawbacks
- when to choose
- patch plan
- test plan

## 8.7 GPT Image 2 visual blueprint generation

### Description

GPT Image 2 generates shareable developer artifacts based on structured output from GPT-5.5.

This is not decorative image generation. It produces useful engineering visuals.

### Required image types

For MVP, generate one of these:

1. **Before/After Architecture Card**
   - Shows current flow vs proposed flow.

2. **Blast Radius Map**
   - Shows affected files/modules and risk intensity.

3. **Branch to the Future Comparison Card**
   - Shows Fast Prototype vs Minimal Patch vs Proper Architecture.

### Example image-generation spec

```json
{
  "artifact_type": "before_after_architecture_card",
  "title": "Async Resume Parsing Impact Plan",
  "before_flow": ["UploadForm", "POST /api/upload", "S3"],
  "after_flow": ["UploadForm", "POST /api/upload", "S3", "Queue", "Parser Worker", "Postgres", "GET /api/upload/status"],
  "risk_nodes": ["S3", "Parser Worker", "Status Polling"],
  "style": "clean technical blueprint, dark background, readable labels, terminal-inspired, developer-focused"
}
```

### Output behavior

- Save generated image to `./branch-to-the-future-artifacts/session-id/blueprint.png`
- Show path in terminal
- Optionally open image in browser
- Optionally show QR/local URL

## 8.8 Export report

### Description

Export the final analysis as Markdown.

### Required sections

- Change request
- Repo summary
- Impact path
- Affected files
- Risk summary
- Branch futures
- Recommended path
- Test plan
- Patch skeleton
- Image artifact link/path

## 9. User interface design

## 9.1 Visual style

Use a polished terminal aesthetic:

- dark background
- box-drawing borders
- minimal accent colors
- status labels
- progress bars
- glowing risk highlights
- no clutter

### Color semantics

- Blue/cyan: scanning, neutral system state
- Green: confirmed safe, completed
- Yellow: candidate, medium confidence
- Orange: likely affected, medium risk
- Red: high risk, must change
- Purple: generated artifact / future path

## 9.2 Navigation

### Global keys

- `q`: quit
- `Esc`: back
- `Tab`: next panel
- `Shift+Tab`: previous panel
- `Enter`: inspect/open
- `r`: replay trace
- `g`: generate image blueprint
- `p`: generate patch skeleton
- `t`: generate test plan
- `e`: export report
- `?`: help

## 9.3 Screens

1. Welcome / Change Input
2. Repo Scan
3. Impact Explorer
4. File Impact Detail
5. Branch to the Future Compare
6. Artifact Generation
7. Export Summary

## 10. Technical architecture

## 10.1 Suggested stack

- Rust
- Ratatui
- Crossterm
- TachyonFX
- ignore or walkdir for file traversal
- serde / serde_json
- tokio for async tasks
- reqwest for OpenAI API calls
- optionally tree-sitter for richer parsing
- optionally ratatui-image for terminal image previews

## 10.2 Modules

```text
src/
  main.rs
  app.rs
  event.rs
  ui/
    mod.rs
    screens/
      input.rs
      scan.rs
      explorer.rs
      file_detail.rs
      futures.rs
      artifact.rs
    widgets/
      repo_tree.rs
      impact_path.rs
      risk_meter.rs
      status_bar.rs
  fx/
    mod.rs
    boot.rs
    scan.rs
    impact.rs
    risk.rs
  repo/
    scanner.rs
    model.rs
    heuristics.rs
  ai/
    openai.rs
    prompts.rs
    schemas.rs
  artifacts/
    image.rs
    markdown.rs
  sample/
    fixtures.rs
```

## 10.3 Runtime data model

```rust
struct AppState {
    repo_path: PathBuf,
    change_request: String,
    screen: Screen,
    repo_model: Option<RepoModel>,
    impact_analysis: Option<ImpactAnalysis>,
    selected_file_index: usize,
    selected_future_index: usize,
    scan_events: Vec<ScanEvent>,
    active_effects: Vec<EffectHandle>,
}

struct RepoModel {
    name: String,
    frameworks: Vec<String>,
    files: Vec<RepoFile>,
    routes: Vec<RouteInfo>,
    tests: Vec<String>,
    config_files: Vec<String>,
    risk_signals: Vec<RiskSignal>,
}

struct RepoFile {
    path: String,
    kind: FileKind,
    size: usize,
    symbols: Vec<String>,
    imports: Vec<String>,
    snippets: Vec<String>,
}

struct ImpactAnalysis {
    summary: String,
    impact_path: Vec<ImpactFile>,
    risk_summary: Vec<String>,
    tests_to_add: Vec<String>,
    futures: Vec<ImplementationFuture>,
    recommended_future: String,
}

struct ImpactFile {
    path: String,
    reason: String,
    impact_score: u8,
    confidence: u8,
    risk: RiskLevel,
    change_needed: String,
}

struct ImplementationFuture {
    name: String,
    description: String,
    complexity: Complexity,
    risk: RiskLevel,
    affected_files: Vec<String>,
    benefits: Vec<String>,
    drawbacks: Vec<String>,
    patch_plan: Vec<String>,
    test_plan: Vec<String>,
}
```

## 11. AI prompt design

## 11.1 GPT-5.5 system prompt

```text
You are a senior software architect and change impact analyst. Your job is to predict the likely blast radius of a proposed code change using the provided repository summary. Do not invent files that are not present unless clearly marked as proposed new files. Prioritize practical reasoning, implementation tradeoffs, risks, and tests. Return strictly valid JSON matching the requested schema.
```

## 11.2 GPT-5.5 user prompt structure

```text
Repository summary:
{repo_model_json}

Proposed change:
{change_request}

Task:
1. Identify the likely impacted files.
2. Explain why each file is affected.
3. Assign impact_score, confidence, and risk.
4. Create an ordered impact path.
5. Identify test changes.
6. Compare three implementation futures:
   - Fast Prototype
   - Minimal Patch
   - Proper Architecture
7. Recommend one path.
8. Return strictly valid JSON.
```

## 11.3 GPT Image 2 prompt builder

Use GPT-5.5 to produce a structured visual spec first. Then pass the spec to GPT Image 2.

Image prompt template:

```text
Create a clean developer-focused architecture card.

Title: {title}

Purpose:
Show the before and after architecture for this proposed code change.

Before flow:
{before_flow}

After flow:
{after_flow}

Affected files/modules:
{affected_files}

Risk areas:
{risk_areas}

Style:
Dark technical blueprint, crisp labels, high readability, terminal-inspired accents, professional engineering artifact, not fantasy, not decorative, no tiny illegible text.
```

## 12. Sample repository for demo

Create a bundled sample repo to guarantee a good demo.

### Suggested sample app

A resume interview app:

```text
sample-repo/
  package.json
  app/
    page.tsx
    api/
      upload/
        route.ts
      feedback/
        route.ts
  components/
    UploadForm.tsx
    FeedbackPanel.tsx
  lib/
    s3.ts
    parser.ts
    openai.ts
  workers/
    parser.ts
  db/
    schema.sql
  tests/
    upload.test.ts
```

### Built-in demo changes

1. `add async resume parsing with S3 upload and status polling`
2. `add Stripe subscription webhook with idempotency`
3. `add team comments with real-time notifications`

Each of these should produce a rich impact path.

## 13. MVP development plan

## Phase 1: Static TUI skeleton

- Create Ratatui app shell
- Add screens and navigation
- Add mock repo model
- Add mock impact analysis
- Render static explorer and futures screens

## Phase 2: Animation layer

- Add TachyonFX
- Implement boot animation
- Implement repo materialization
- Implement impact trace reveal
- Implement risk pulse
- Add replay trace command

## Phase 3: Real repo scanner

- Traverse local repo
- Build file tree
- Extract basic metadata
- Detect frameworks
- Detect routes/tests/configs
- Build compact JSON repo model

## Phase 4: GPT-5.5 integration

- Send repo model + change request
- Parse structured JSON response
- Replace mock analysis with real analysis
- Add fallback response if API fails

## Phase 5: GPT Image 2 artifacts

- Generate image spec
- Generate architecture card
- Save artifact locally
- Show path in terminal
- Optional browser open

## Phase 6: Polish

- Add demo sample repo
- Add export Markdown
- Add error handling
- Add loading states
- Add keyboard help
- Add final demo script

## 14. Demo script

### Opening line

“Most AI coding tools jump straight into writing code. Branch to the Future does the step before that: it simulates the future of a code change so developers can understand the blast radius first.”

### Demo command

```bash
branch-to-the-future ./sample-repo
```

### Change request

```text
add async resume parsing with S3 upload and status polling
```

### Narration

1. “The tool scans the repo and builds a lightweight map of files, routes, configs, and tests.”
2. “Now it animates the impact exploration. It starts from the UI upload component, follows the API route, finds storage, infers the worker layer, and checks database impact.”
3. “Instead of a single answer, it compares three possible futures: fast prototype, minimal patch, and proper architecture.”
4. “GPT-5.5 does the codebase reasoning. GPT Image 2 turns the selected future into a shareable architecture card.”
5. “The goal is not to replace the developer. It is to give them a map before they start cutting into the codebase.”

### Closing line

“Branch to the Future helps developers see the blast radius before they ship the blast.”

## 15. Success metrics

### MVP success

- User can scan a local sample repo.
- User can enter a change request.
- App displays animated impact exploration.
- App shows affected files with reasoning.
- App compares three implementation paths.
- App generates a test plan and patch plan.
- App generates or mocks a visual blueprint artifact.

### Hackathon success

- Judges understand the product within 30 seconds.
- Animation visibly communicates codebase exploration.
- GPT-5.5 reasoning output feels specific to the repo.
- GPT Image 2 output is a useful developer artifact.
- Demo works without internet fallback issues where possible.

## 16. Risks and mitigations

### Risk: Real repo analysis is too hard

Mitigation:
Use heuristics plus GPT reasoning. Keep a bundled sample repo for demo.

### Risk: GPT JSON output fails

Mitigation:
Use strict schemas, retry once, and have a mock fallback.

### Risk: Animation takes too long to build

Mitigation:
Start with mock timed events. The animation does not need to reflect real-time scanning exactly.

### Risk: Image generation is slow

Mitigation:
Allow pre-generated demo image fallback. Still show the generation step if API works.

### Risk: Terminal image display is difficult

Mitigation:
Save image to file and open in browser. The TUI is the command center, not the image viewer.

## 17. Stretch features

- Git diff mode: compare current branch against main
- PR review mode: simulate blast radius of a pull request
- Test generation with runnable test files
- Patch skeleton generation with file-by-file diffs
- Interactive graph navigation
- Terminal-based mini architecture diagram
- Local model fallback
- GitHub Actions integration
- Export to Notion/Markdown/GitHub issue
- Watch mode for live repo changes
- Configurable animation themes
- `branch-to-the-future replay session.json`

## 18. Open questions

1. Should the MVP focus on one language/framework or support many shallowly?
2. Should the image artifact be required in the demo or optional?
3. Should patch skeleton generation be included in MVP or saved for stretch?
4. Should the app work fully offline with mock mode?
5. Should “Branch to the Future” be the final name or just the internal codename?

## 19. Recommended MVP scope for hackathon

Build exactly this:

1. Ratatui app with TachyonFX animations
2. Bundled sample repo
3. Natural-language change input
4. Mock or real repo scanner
5. GPT-5.5 impact analysis
6. Animated Impact Explorer
7. Three Branch to the Future comparison
8. Test plan + patch plan
9. GPT Image 2 blueprint generation
10. Markdown export

Do not build:

- Authentication
- Cloud dashboard
- GitHub OAuth
- Full automatic patching
- Multi-language static analysis perfection

## 20. Final MVP description

Branch to the Future is a terminal-native change impact simulator for developers. It lets a developer describe a planned feature or refactor, then scans the repo and animates the exploration of affected files. GPT-5.5 predicts the blast radius, explains why each file is involved, compares multiple implementation paths, and generates a test and patch plan. GPT Image 2 turns the selected future into a shareable visual architecture card. The result is a cinematic developer tool that helps engineers understand what a change will touch before they start coding.

