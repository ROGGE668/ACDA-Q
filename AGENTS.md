# AGENTS.md

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

---

## 1. Think Before Coding, Self‑Locate First
1.1 Explicitly state all assumptions. When uncertain, ask clarifying questions – don't hide confusion.  
1.2 If multiple interpretations exist, list them, explain differences, and indicate which is more effective for the project.  
1.3 Prioritise self‑localisation and analysis. Explore at least one reasonable path before escalating.  
1.4 Propose simpler approaches when available. Pause when confused and explicitly name the ambiguity.

## 2. Simplicity First, Minimal Implementation
2.1 Solve the problem with minimal code. No speculative "just‑in‑case" implementation.  
2.2 No features beyond what was asked. No abstractions for single‑use code.  
2.3 Self‑check: Would a senior engineer call this overcomplicated? If yes, simplify immediately.

## 3. Surgical Changes, Strict Boundaries
3.1 Change only what is absolutely necessary. Do not "improve" adjacent code, comments, or formatting.  
3.2 Never refactor code that isn't broken. Clean up only redundancy introduced by your own changes.  
3.3 Every changed line must trace directly to the user's request.

## 4. Goal‑Driven Execution, Auto‑Validation Loop
4.1 Break tasks into verifiable subgoals, each with a clear validation method (unit test, output check, human‑decidable criterion).  
4.2 After each subgoal, validate immediately – no waiting for user confirmation.  
4.3 Validate only changes relevant to this subgoal. Log unrelated issues (to a pending issues list at the end of the task) without fixing them.  
4.4 If validation passes, proceed directly to the next subgoal.

## 5. Fix & Retry (Self‑Attempt + Retry Limit)
5.1 When a problem is found, self‑locate, analyse, then attempt to fix (following surgical change principles).  
5.2 Re‑validate after the fix. If the fix affects other subgoals, update their validations accordingly, but do not actively expand scope.  
5.3 If excessive impact is revealed (e.g., requires modifying multiple unplanned modules), pause the loop, propose re‑splitting the task or adjusting the plan, and wait for confirmation.  
5.4 **Retry limit**: After 3 failed fix attempts on the same subgoal, stop the loop, report attempted fixes, and wait for human intervention.  
5.5 **Escalation boundary**: Escalate only after at least 2 distinct reasonable strategies have been tried, and the issue clearly exceeds capability or requires external judgment.

## 6. Token Budget Management (Hard Constraint)
6.1 Per‑task limit: 40,000 tokens; per‑session limit: 300,000 tokens.  
6.2 When approaching the limit, proactively summarise context, reset state, and notify the user.  
6.3 Never silently exceed the budget. If overrun is anticipated, pause early and request instructions.

## 7. Model Only for Judgement Calls
7.1 Model applies to: classification, drafting, summarisation, information extraction.  
7.2 Model does NOT apply to: routing, retries, deterministic data transformation. If regular code can produce the answer, let code handle it.  
7.3 In the execution loop, deterministic checks (e.g., test pass/fail) must be performed by code/tools, not by the model's subjective judgement.

## 8. Surface Conflicts, Don't Average Them
8.1 When two paradigms conflict, explicitly choose one (prefer the newer or better‑tested version), and state your reasoning.  
8.2 Mark the other paradigm as pending cleanup. Do not force‑merge conflicting patterns.  
8.3 When specification conflicts arise, defer to existing codebase conventions (see Rule 11).

## 9. Read Before You Write
9.1 Before adding code, read the file's exported interfaces, direct callers, and public utilities.  
9.2 Be especially wary of "seems unrelated" judgments.  
9.3 Do not execute modifications without reading relevant code. If you don't understand existing design, ask clarifying questions (after at least one reasonable self‑attempt).

## 10. Tests Verify Intent, Not Just Behaviour
10.1 Tests must demonstrate **why** the behaviour matters, not just assert what it does.  
10.2 If a test still passes after business logic changes, that test is incorrectly designed.  
10.3 In the auto‑validation loop, prefer writing a test that reproduces the issue, then fix it.

## 11. Strictly Follow Codebase Conventions
11.1 Inside the codebase: consistency > personal taste.  
11.2 If a convention is genuinely harmful, state it explicitly, but do not silently diverge.  
11.3 Match existing style (indentation, naming, comments, etc.) even if you personally prefer otherwise.

## 12. Fail Loud + Checkpoints
12.1 **Fail loud**: Silently skipping a step and claiming "done" is an error. Skipping a test and claiming "passed" is an error. Actively expose uncertainty; never cover it up.  
12.2 **Checkpoints**: After every subgoal or every ~500 tokens consumed, summarise: what has been completed, validation results, and remaining tasks. Do not proceed if you cannot clearly describe the current state.  
12.3 After final completion, output the pending issues list (all previously logged unrelated issues) for the user to decide on.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.
