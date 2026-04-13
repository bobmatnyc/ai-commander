# Spurious Inline Buttons on Informational Messages

**Date**: 2026-04-06
**Status**: Analysis complete -- bug pattern identified

## Two Independent Button-Generation Paths

There are **two separate detection systems** that attach buttons to messages. Both are potential sources of spurious buttons.

### Path 1: `OptionDetector::detect_options` (on COMPLETED output)

**File**: `crates/commander-core/src/options.rs`
**Called at**: `crates/commander-telegram/src/bot.rs:595` inside the `PollResult::Complete` handler

When a session completes, the final response text is scanned by `OptionDetector::detect_options()`. If it matches, an `InlineKeyboardMarkup` is created via `create_option_keyboard()` (bot.rs:298) and attached to the final message via `send_long_message()` (bot.rs:602, param at bot.rs:609).

**THIS IS THE PRIMARY BUG LOCATION.** The detector triggers on:

- **Numbered lists** (`detect_number_options`, options.rs:159): regex `^(\d+)[\)\.]\s*(.+?)$` with sequential check. Any informational summary like "Here is what I did:\n1. Created file\n2. Updated config\n3. Ran tests" will match -- sequential numbers starting from 1 with period-delimited text.
- **Lettered lists** (`detect_letter_options`, options.rs:100): regex `^([A-Za-z])[\)\.]\s*(.+?)$` with sequential letter check (A, B, C...). Summary output like "A. Authentication module\nB. Backend service" would match.
- **Y/N patterns** (`detect_yes_no`, options.rs:66): regex for `(y/n)` or `(yes/no)` anywhere in text. Less likely to be spurious but could match quoted text.

The detector has **no way to distinguish** between "Claude asking the user to choose" vs "Claude reporting what was done using a numbered list." It has no context about whether the session is soliciting input or delivering results.

### Path 2: `detect_selector` (on LIVE terminal output)

**File**: `crates/commander-core/src/output_filter.rs:498`
**Called at**: `crates/commander-telegram/src/state.rs:1293` and `state.rs:2008`
**Consumed at**: `crates/commander-telegram/src/bot.rs:660`

This detects **interactive terminal selectors** (Inquirer.js arrow-key pickers, numbered prompts in the live PTY). Buttons are created inline at bot.rs:701-713 and sent with `reply_markup` at bot.rs:718.

This path is **less likely to cause spurious buttons** because:
- Pattern 1 (arrow selector) requires literal `>` or unicode arrow characters
- Pattern 2 (numbered) uses the same weak heuristic as Path 1 but runs against raw terminal output, which is more likely to contain actual selector prompts
- Selector messages are tracked (`selector_messages` HashMap) and deleted on session completion (bot.rs:564)

However, the numbered-list detector in `detect_selector` (output_filter.rs:599-654) has the **same false-positive problem**: it will match any numbered list in the terminal output window.

## All Button-Attachment Points

| Location | File:Line | Trigger | Legitimate? |
|---|---|---|---|
| `create_option_keyboard` | bot.rs:298 | `PollResult::Complete` with detected options | **OFTEN SPURIOUS** -- fires on informational numbered/lettered lists |
| `send_long_message` | bot.rs:361,394 | Passes keyboard from Complete handler | Passthrough only |
| Selector keyboard | bot.rs:701-718 | `PollResult::SelectorDetected` | Usually legitimate (Inquirer.js) |

## Root Cause

`OptionDetector::detect_options()` in `options.rs` runs on every completed response and uses only syntactic pattern matching (sequential numbered/lettered lines). It cannot distinguish between:
- "Which approach do you prefer?\n1. Refactor\n2. Rewrite" (actual choice)
- "Done! Here's what I completed:\n1. Fixed the bug\n2. Added tests\n3. Updated docs" (informational summary)

## Recommended Fix

Add a heuristic to `detect_number_options` and `detect_letter_options` that requires the text preceding the options to contain question-like language (question mark, "choose", "select", "prefer", "which", "how would you like"). Alternatively, check whether the numbered items appear at the **end** of the message (choice prompt) vs embedded within a larger narrative (informational).
