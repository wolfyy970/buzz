# Impeccable: desktop product-review gaps

Impeccable's critique is strong at reviewing a single surface, but it can miss
basic product hierarchy when the real unit of review is a desktop workflow.
That happened in Buzz's agent-template editor: the generic action title became
more prominent than the agent being edited.

## Changes worth contributing

1. **Add a desktop application mode.** Detect Tauri and Electron separately
   from websites and native mobile apps. Calibrate density, spacing, window
   resizing, keyboard behavior, focus, and modal conventions for desktop
   software.
2. **Review flows as well as screens.** Accept a sequence of named states or
   screenshots and assess the complete task: entry point, edit, save, publish,
   impact confirmation, success, failure, and recovery.
3. **Add an object–action hierarchy check.** In editors, settings, and
   inspectors, verify that the object and current context are immediately
   identifiable. Generic actions such as “Edit template” should not outrank
   the name of the agent being edited.
4. **Inspect the incumbent design system first.** Identify the app's type
   scale, spacing tokens, component vocabulary, and comparable dialogs before
   scoring an isolated surface.
5. **Use the application's own rendering harness.** A Tauri app may not render
   correctly in a plain browser. Critique should support project-provided
   screenshot commands and require representative states rather than infer the
   interface from source alone.
6. **Require a compact hierarchy pass before the full heuristic report.** Check
   page title, entity/context, section order, primary action, destructive
   action, status, and error placement. These basic failures should be caught
   before stylistic observations.
7. **Test desktop interaction states.** Include keyboard traversal, Escape and
   close behavior, unsaved changes, minimum window size, scrolling under a
   fixed footer, disabled/loading actions, and error recovery.

## Suggested shape

Add a `critique-flow` target manifest containing the platform, task, ordered
states, source files, and screenshot commands. Keep the existing independent
visual and detector assessments, but add a third task-flow assessment that has
to explain what the user believes they are editing, what changes now, and what
changes only after publishing.
