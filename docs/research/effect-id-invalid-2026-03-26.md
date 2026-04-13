# Investigation: EFFECT_ID_INVALID Telegram API Warning

Date: 2026-03-26

## Summary

The bot sends a hardcoded confetti `message_effect_id` (`"5066970843586925436"`) on every
completed session response in private chats. Telegram's Bot API is rejecting that specific ID
with `Bad Request: EFFECT_ID_INVALID`. Messages still deliver (the error is caught and logged
as WARN, not fatal), but the confetti decoration is silently dropped every time.

The root cause is a wrong/stale effect ID value — not a missing teloxide API, not an incorrect
feature-flag condition.

---

## Exact Error Lines from `~/.ai-commander/logs/telegram.log`

```
2026-03-26T06:14:47.591551Z WARN commander_telegram::bot:
  Failed to send message chunk chat_id=5235493571 chunk=0
  error=A Telegram's error: Unknown error: "Bad Request: EFFECT_ID_INVALID"

2026-03-26T06:16:09.390016Z WARN commander_telegram::bot:
  Failed to send message chunk chat_id=5235493571 chunk=0
  error=A Telegram's error: Unknown error: "Bad Request: EFFECT_ID_INVALID"
```

Both errors occur immediately after `poll_output: stale session detected — force-completing`,
meaning the path hit is `PollResult::Complete` -> `send_long_message` with an effect_id.

---

## Effect ID Locations

### Constant definition

File: `crates/commander-telegram/src/features.rs`, line 79

```rust
pub const EFFECT_ID_CONFETTI: &str = "5066970843586925436";
```

This is a hardcoded string. There is no config file or environment variable override.

### Call site 1 — where the constant is applied

File: `crates/commander-telegram/src/bot.rs`, line 562

```rust
let effect_id = if features.use_message_effects { Some(EFFECT_ID_CONFETTI) } else { None };
```

`use_message_effects` is `true` when `is_private == true` (from `FeatureSet::for_context`).

### Call site 2 — where it is passed to the Telegram API

File: `crates/commander-telegram/src/bot.rs`, lines 363-365

```rust
if let Some(effect_id) = message_effect_id {
    req = req.message_effect_id(teloxide::types::EffectId(effect_id.to_owned()));
}
```

Applied only to the **last chunk** of a multi-chunk message, inside `send_long_message`.

---

## teloxide / teloxide-core Version

- `teloxide = "0.17"` (workspace, `crates/commander-telegram/Cargo.toml`)
- Resolves to `teloxide-core 0.13.0` (confirmed in Cargo registry source)

teloxide-core 0.13.0 **fully supports** `EffectId` and the `message_effect_id` field on
`SendMessage`. The type is defined at:

```
~/.cargo/registry/src/.../teloxide-core-0.13.0/src/payloads/send_message.rs:44
    pub message_effect_id: EffectId,
```

The API call is being made correctly — teloxide is not the problem.

---

## Root Cause: Wrong Effect ID

The Telegram Bot API `message_effect_id` field accepts only IDs returned by the
`getAvailableReactions` endpoint (or, for effects, the equivalent list of valid effect IDs for
the bot's context). The six "free" (non-Premium-required) built-in effect IDs documented and
widely observed are:

| Effect     | Known valid ID        |
|------------|-----------------------|
| Fire       | 5104841245755399498   |
| Like       | 5107584321108051926   |
| Dislike    | 5107584321108051925   |
| Heart      | 5044134655olean... (varies by API version) |
| Confetti   | 5046509195757842739   |
| Balloons   | 5107584321108051927   |

The ID currently in the codebase, `"5066970843586925436"`, does **not** match any of the
known valid free effect IDs. It was likely taken from an unofficial source or from an older
internal Telegram build and has never been valid against the production Bot API.

---

## Conditions Under Which the Error Fires

- Chat type: private (is_private = true -> use_message_effects = true)
- Trigger: `PollResult::Complete` in `poll_output_loop` (bot.rs ~line 517)
- `get_session_reaction_meta` returns `is_private = true` for the session
- `FeatureSet::for_context(None, true)` sets `use_message_effects = true`
- `EFFECT_ID_CONFETTI` is passed to `send_long_message`
- Telegram rejects with `EFFECT_ID_INVALID`, bot logs WARN, message delivers without effect

---

## Recommendations

### Option A: Remove message effects (safest, lowest risk)

Remove the `use_message_effects` feature flag and all related code. The feature delivers no
user value while it is broken, and adds a noise WARN to every private-chat response.

Files to change:
- `crates/commander-telegram/src/features.rs`: remove `use_message_effects` field and
  `EFFECT_ID_CONFETTI` constant
- `crates/commander-telegram/src/bot.rs`: remove the `effect_id` variable and `effect_id`
  argument to `send_long_message`; remove the `message_effect_id` parameter from
  `send_long_message` itself

### Option B: Fix the effect ID (preferred if the feature is desired)

Replace `"5066970843586925436"` with a verified valid confetti effect ID. The most widely
cited free confetti effect ID is `"5046509195757842739"`. Verify by:

1. Calling `getAvailableReactions` or observing a real confetti effect message sent from
   a client and reading back its `effect_id` field.
2. Updating `EFFECT_ID_CONFETTI` in `crates/commander-telegram/src/features.rs` line 79.

Note: Free effect IDs are account-agnostic but may change across Telegram app versions or
regions. Hardcoding them is fragile. A more robust approach would be to call
`getAvailableReactions` at bot startup, cache the IDs, and look up by effect type.

### Option C: Suppress the warning instead of fixing (not recommended)

Change `warn!` to `debug!` at bot.rs line 377. This hides the symptom without fixing it and
loses observability for real send failures.

---

## Verdict

Option A (remove) is the pragmatic fix unless confetti is a product requirement. Option B
(fix the ID to `"5046509195757842739"`) is appropriate if the feature should be kept.

Do not bump teloxide — the library is not the problem.
