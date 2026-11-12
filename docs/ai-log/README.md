# AI log

This directory records the interaction between the human maintainers and the
AI coding agent that implements the tasks of `IMPLEMENTATION.md`.

## Convention

- One file per task, named after the task ID: `F1.md`, `A3.md`, and so on.
  Tasks of phase N are logged in `igs-iso9506-mms`, where they are made; tasks
  of phase P are logged in the private `norm_preparation` repository.
- Each file contains the prompt sent for that task, then a short summary of
  what the agent produced (files touched, tests added, anything a reviewer
  should look at).
- **No text from the IEC and ISO standards**, nor from the IEC code components
  or the TISSUE database, in these files. Cite clauses and paraphrase (see the
  rules in `IMPLEMENTATION.md` section 1).
- The agent settles ambiguities itself (rule 10). Decisions go to
  `PROVENANCE.md`, with their confidence and a test; this log only points to them.
