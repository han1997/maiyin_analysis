# UI Redesign Audit

## Product register and scene

- Register: product tool, not a brand or marketing surface.
- Physical scene: internal review staff use a Windows office computer in daytime and inspect dense identity and accommodation tables for long periods.
- Theme implication: light, low-stimulation surfaces with cool tinted neutrals and a restrained dark-blue accent.
- Accessibility target: WCAG 2.1 AA; keyboard focus must be visible and risk states must combine text, iconography, and color.

## Existing interaction model

The Tkinter application already has a recognizable desktop workflow:

- Top actions for template download and clearing data.
- Fixed left control area for import, history, storage directory, and analysis parameters.
- Adaptive right workspace for summary, search/filter controls, results table, pagination, and exports.
- Separate detail and parameter windows.
- Native file/folder dialogs and background processing feedback.

This information architecture should be preserved where it helps experienced users. The redesign should improve hierarchy and progressive disclosure instead of inventing a different navigation model.

## Weak points to address in the Tauri UI

- Tkinter layout is implemented in one large application class, so visual states and behavior are difficult to reason about independently.
- Important states are mostly message boxes; the new UI needs inline import progress, validation, empty, partial-failure, and export feedback.
- The result table needs stronger scan hierarchy, sticky controls/header, stable column sizing, tabular figures, and explicit risk labels.
- Analysis parameters are numerous; the default view should show the active scope summary while a focused panel exposes advanced filters.
- History selection and merged-analysis state need clearer distinction so clearing a combined view cannot be confused with deleting saved sessions.
- Sensitive data needs a persistent local-processing assurance without turning it into decorative branding.

## Proposed visual direction

- Windows-native product feel using `Microsoft YaHei UI`, `Segoe UI`, and system fallbacks.
- Restrained color strategy expressed as OKLCH design tokens derived from the original cool gray, white surface, dark ink, muted text, dark blue accent, and red danger role.
- One application shell: compact top bar, resizable/collapsible left control rail, flexible results workspace, and a right-side detail inspector rather than modal-first navigation.
- Flat surfaces separated mainly by spacing, 1 px borders, and subtle tonal layers. Avoid nested cards, heavy shadows, gradients, glass effects, and ornamental motion.
- 150-220 ms state transitions using opacity and transform only; respect `prefers-reduced-motion`.
- Desktop-first responsive behavior: at narrower widths the left rail becomes a drawer and the detail inspector becomes a full-width view; the data table retains horizontal scrolling rather than crushing columns.

## Implementation implications

- Define reusable tokens and primitives before assembling the main screen.
- Separate import, analysis, filtering, persistence, and export logic from React views.
- Model loading, empty, error, success, disabled, selected, and destructive-confirmation states explicitly.
- Keep realistic Chinese sample data for a browser preview, but make sample mode visibly distinct from imported local data.

