# CLAUDE.md

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

---

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

Before analysis, explicitly state all assumptions. If anything is unclear, ask – don't hide confusion.

Explain the assumptions. If multiple options exist, explain the differences between them and indicate which one is more effective for the project.

Consider multiple possible interpretations or alternatives; if a simpler approach than the current assumption exists, propose it.

Break the task into verifiable subgoals, each with a clear validation method (e.g., unit test, output check, human-decidable criterion).

**Special case:** If a subgoal cannot be automatically verified (e.g., subjective UI experience, external dependency) or verification costs are too high, pause and request human confirmation – do not proceed blindly.

---

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

---

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

---

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

## 5. Iterative Execution Loop

### Step 1: Plan before each iteration
Before each iteration, restate: the current subgoal, what has been completed, and the next target.

### Step 2: Execute and validate immediately
After completing each subgoal, check it immediately (no waiting for confirmation):
- Validate against the success criteria defined for that subgoal (e.g., test passes, output matches expectation).
- Check only changes relevant to this subgoal; if unrelated issues are noticed, only log them (to a pending issues list accumulated at the end of the task) – do not fix them.
- If validation passes, proceed directly to the next subgoal.

### Step 3: Fix problems before continuing
If a problem is found, fix it first, then continue:
- **Fixing principles:** surgical changes – modify only the minimal code/config that caused the failure. Do not improve adjacent code or refactor things that aren't broken. After the fix, re-check the subgoal to ensure it passes before moving on.
- **Scope control:** If the fix affects other subgoals, update their validations accordingly, but do not actively expand the scope of the fix.
- **Impact guard:** If the fix reveals excessive impact (e.g., requires modifying multiple unplanned modules), pause the loop, propose re-splitting the task or adjusting the plan, and wait for confirmation.
- **Retry limit:** If the same subgoal fails to be fixed after 3 attempts (or a task-appropriate limit), stop the loop, report the problem and attempted fixes, and wait for human intervention.

### Step 4: Repeat until complete
Repeat steps 1-3 until all tasks are completed:
- Keep implementation minimal: no features beyond what was asked, no unnecessary abstractions or error handling for impossible scenarios.
- After final completion, verify that all success criteria are met (e.g., all tests pass, the original problem is resolved), and output the pending issues list (previously logged unrelated issues) for the user to decide whether to address.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.
