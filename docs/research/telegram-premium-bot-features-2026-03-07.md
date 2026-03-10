# Telegram Premium Bot Features Research

**Date:** 2026-03-07
**Scope:** Features available to bot developers when the bot owner or users have Telegram Premium
**API Versions Covered:** Bot API 7.0 through 8.x (December 2023 - early 2026)
**Sources:** core.telegram.org/bots/api, core.telegram.org/bots/api-changelog, official Telegram blog

---

## Summary

Telegram Premium unlocks two distinct categories of bot features:

1. **Bot-owner Premium gating** - Features the bot itself can USE when the bot's owner account has Premium
2. **User Premium detection** - Features bots can OFFER differently based on whether the calling user has Premium

Most features degrade gracefully: standard emoji show instead of custom emoji; reactions fall back to standard; formatting falls back to plain text. The most impactful Premium-gated categories for bot developers are custom emoji, message effects, business account integration, and stories.

---

## Feature-by-Feature Analysis

### 1. Stickers and Emoji

#### 1.1 Custom Emoji in Messages

| Attribute | Detail |
|-----------|--------|
| Feature | Send animated custom emoji inline in message text/captions |
| API method/parameter | `MessageEntity` type `custom_emoji` with field `custom_emoji_id`; pass in `entities` array of send methods |
| Helper method | `getCustomEmojiStickers(custom_emoji_ids)` to fetch sticker info |
| Premium requirement | **Bot owner must have Telegram Premium** (OR bot has purchased a Fragment username) |
| Graceful degradation | Standard Unicode emoji is shown on clients that cannot render custom emoji; in system notifications and non-premium forwards, the fallback Unicode character appears automatically |
| Notes | All users can see custom emoji already in messages - they just cannot send new ones without Premium. Saved Messages chat lets any user try custom emoji for free |

#### 1.2 Premium Emoji Sticker Sets

| Attribute | Detail |
|-----------|--------|
| Feature | Access and send stickers from Premium packs |
| API method | `sendSticker`, `getSticker`, `getStickerSet` |
| Premium requirement | No special requirement to SEND stickers from Premium packs via bots - packs are readable by all bots |
| Notes | Sticker type field `sticker_type` distinguishes `regular`, `mask`, `custom_emoji`; `is_premium` field on Sticker objects marks Premium-exclusive animated stickers |

#### 1.3 Video Stickers (.WEBM)

| Attribute | Detail |
|-----------|--------|
| Feature | Upload and send .WEBM format video stickers |
| API method | `uploadStickerFile`, `createNewStickerSet`, `addStickerToSet` with `format` field in `InputSticker` |
| Premium requirement | None - available to all bots |
| Notes | Added in Bot API 7.2; mixed sticker packs now supported, max 120 stickers per set |

---

### 2. File Uploads

#### 2.1 Large File Support (up to 4 GB)

| Attribute | Detail |
|-----------|--------|
| Feature | Users with Premium can send files up to 4 GB (vs 2 GB standard) |
| API method | Standard send methods: `sendDocument`, `sendVideo`, `sendAudio`, etc. |
| Bot API server | Using a **local Bot API server** enables uploading files up to 2,000 MB and downloading without size limit. The cloud Bot API server has lower limits |
| Premium requirement | **User must have Premium** for 4 GB uploads. Bots receive these files if the user sends them. To send 4 GB files TO users, use a local Bot API server |
| Graceful degradation | Standard 2 GB limit applies for non-premium users; bots should check file size before processing |
| Detection | Check `file_size` on received `Document`, `Video`, `Audio` objects; check `User.is_premium` to know if a user can send large files |

---

### 3. Formatting

#### 3.1 Blockquotes

| Attribute | Detail |
|-----------|--------|
| Feature | Render text as a visual blockquote (indented/highlighted block) |
| API method | `MessageEntity` type `blockquote`; use `>` in MarkdownV2, `<blockquote>` in HTML parse mode |
| Premium requirement | **None** - blockquotes work for all bots sending messages |
| Added | Bot API 7.0 (December 2023) |
| Graceful degradation | Clients that do not support blockquote show text without formatting |

#### 3.2 Expandable Blockquotes

| Attribute | Detail |
|-----------|--------|
| Feature | Blockquotes that are collapsed by default with an expand button |
| API method | `MessageEntity` type `expandable_blockquote`; `<blockquote expandable>` in HTML, `**>` in MarkdownV2 |
| Premium requirement | **None** |
| Added | Bot API 7.4 (May 2024) |
| Graceful degradation | Older clients show as a regular blockquote or plain text |

#### 3.3 Spoilers

| Attribute | Detail |
|-----------|--------|
| Feature | Hide text behind a tap-to-reveal spoiler overlay |
| API method | `MessageEntity` type `spoiler`; `||text||` in MarkdownV2, `<tg-spoiler>` in HTML |
| Premium requirement | **None** - spoilers are available to all bots |
| Graceful degradation | Non-supporting clients show the text directly |

#### 3.4 Text Quotes (Reply Quotes)

| Attribute | Detail |
|-----------|--------|
| Feature | When replying, quote a specific portion of the original message |
| API parameter | `ReplyParameters.quote` and `ReplyParameters.quote_entities` |
| Premium requirement | **None** |
| Added | Bot API 7.0 |

---

### 4. Reactions

#### 4.1 Standard Emoji Reactions

| Attribute | Detail |
|-----------|--------|
| Feature | Bots can set emoji reactions on messages |
| API method | `setMessageReaction(chat_id, message_id, reaction, is_big)` |
| Reaction object | `ReactionTypeEmoji` with field `emoji` (standard emoji character) |
| Premium requirement | **None** - all bots can set standard emoji reactions |
| Added | Bot API 7.0 |
| Notes | Bots do NOT receive `MessageReactionUpdated` updates for their own reactions. They DO receive it for non-anonymous user reactions. Reactions must be in the chat's allowed reaction list |

#### 4.2 Custom Emoji Reactions

| Attribute | Detail |
|-----------|--------|
| Feature | React with an animated custom emoji instead of standard emoji |
| API method | `setMessageReaction` with `ReactionTypeCustomEmoji` containing `custom_emoji_id` |
| Premium requirement | **Two conditions apply:** (a) if a Premium user already added that custom emoji reaction, any bot can add the same one; (b) if chat admin explicitly allows custom emoji reactions, bots may use them freely |
| Added | Bot API 7.0 |
| Graceful degradation | Fall back to `ReactionTypeEmoji` with a standard emoji |

#### 4.3 Paid Reactions

| Attribute | Detail |
|-----------|--------|
| Feature | Reactions backed by Telegram Stars ("paid reactions") |
| API class | `ReactionTypePaid` |
| Premium requirement | Users send paid reactions (costs Stars); bots can READ them in `MessageReactionUpdated` updates |
| Added | Bot API 7.9 |
| Notes | Primarily a user-facing feature; bots observe paid reactions in updates |

#### 4.4 Reaction Tracking

| Attribute | Detail |
|-----------|--------|
| Feature | Get counts of all reactions on a message |
| API update | `MessageReactionCountUpdated` (public channels) and `MessageReactionUpdated` (non-anonymous reactions in groups) |
| Premium requirement | None |
| Field | `max_reaction_count` in `ChatFullInfo` shows the limit per user |

---

### 5. Bot Profile

#### 5.1 Bot Profile Photo Management

| Attribute | Detail |
|-----------|--------|
| Feature | Bots can set and remove their own profile photo |
| API methods | `setMyName`, `setMyDescription`, `setMyShortDescription` for text fields; profile photo via BotFather |
| Premium requirement | None for basic profile photo |
| Notes | Business account profile photos managed via `setBusinessAccountProfilePhoto` / `removeBusinessAccountProfilePhoto` (requires business connection) |

#### 5.2 Animated Profile Photos

| Attribute | Detail |
|-----------|--------|
| Feature | Animated profile photos visible to bots via User objects |
| API field | `User` objects include profile photo accessible via `getUserProfilePhotos` |
| Premium requirement | **Users need Premium** to set animated profile photos; bots can read them |
| Notes | `user_profile_photos` update type surfaced when users change their photo |

#### 5.3 Profile Accent Colors and Backgrounds

| Attribute | Detail |
|-----------|--------|
| Feature | Users with Premium can set custom profile colors and backgrounds |
| API fields | `User.accent_color_id`, `Chat.background_custom_emoji_id` |
| Premium requirement | **User needs Premium** to set these; bots receive them in Chat/User objects |
| Related | `ChatBackground`, `BackgroundType`, `BackgroundFill` classes added in Bot API 7.3 for service messages about background changes |

---

### 6. Voice and Video

#### 6.1 Video Notes (Round Videos)

| Attribute | Detail |
|-----------|--------|
| Feature | Send and receive circular video notes |
| API method | `sendVideoNote(chat_id, video_note, ...)` |
| Premium requirement | None |
| Notes | No specific Premium enhancement for video notes in Bot API |

#### 6.2 Voice Notes

| Attribute | Detail |
|-----------|--------|
| Feature | Send voice notes |
| API method | `sendVoice(chat_id, voice, ...)` |
| Premium requirement | None for standard voice notes |
| Notes | No transcription API available in Bot API (transcription is client-side Premium feature) |

#### 6.3 Premium Video Playback / Enhanced Video Messages

| Attribute | Detail |
|-----------|--------|
| Feature | `cover` and `start_timestamp` fields for videos |
| API fields | `cover` (custom thumbnail), `start_timestamp` (start position), `video_start_timestamp` in forward/copy |
| Added | Bot API 8.3 (February 2025) |
| Premium requirement | None |

**Note:** Voice/video features do not have significant Premium-specific bot API enhancements. Premium's voice-to-text transcription and 4x-speed playback are client-side features invisible to bots.

---

### 7. Business Bots (Telegram Business)

Telegram Business is available to Premium subscribers and is the most feature-rich Premium integration for bots.

#### 7.1 Business Connection

| Attribute | Detail |
|-----------|--------|
| Feature | A Premium user connects a bot to their business account; bot can manage their private chats |
| API class | `BusinessConnection` |
| API updates | `business_message`, `edited_business_message`, `deleted_business_messages` updates received by bot |
| Premium requirement | **User must have Telegram Premium** to use Business features (and thus connect a bot) |
| Added | Bot API 7.2 (March 2024) |
| Setup | User: Settings > Telegram Business > Chatbots > add bot; specify which chats the bot can see |

#### 7.2 Sending as Business Account

| Attribute | Detail |
|-----------|--------|
| Feature | Bot sends messages appearing as if from the business owner |
| API parameter | `business_connection_id` added to all send methods: `sendMessage`, `sendPhoto`, `sendVideo`, `sendDocument`, `sendPaidMedia`, etc. |
| Premium requirement | **User (business owner) must have Premium** |
| Notes | Messages show no bot attribution to end customers |

#### 7.3 Business Account Management

| Attribute | Detail |
|-----------|--------|
| Feature | Full business profile management via Bot API |
| API methods | `setBusinessAccountName`, `setBusinessAccountUsername`, `setBusinessAccountBio`, `setBusinessAccountProfilePhoto`, `removeBusinessAccountProfilePhoto`, `setBusinessAccountGiftSettings`, `getBusinessAccountStarBalance`, `readBusinessMessage`, `deleteBusinessMessages` |
| Premium requirement | **User (account owner) must have Premium**; bot must have business connection |
| Added | Bot API 8.x (2024-2025) |

#### 7.4 Pinning Messages for Business

| Attribute | Detail |
|-----------|--------|
| Feature | Bot pins/unpins messages in business chats |
| API methods | `pinChatMessage(business_connection_id=...)`, `unpinChatMessage(business_connection_id=...)` |
| Premium requirement | Active business connection (Premium user) |
| Added | Bot API 7.8 |

#### 7.5 Invoice Links for Business

| Attribute | Detail |
|-----------|--------|
| Feature | Create payment invoice links on behalf of business account |
| API method | `createInvoiceLink(business_connection_id=...)` |
| Premium requirement | Active business connection |
| Added | Bot API 7.9 |

---

### 8. Chat Boosts

#### 8.1 Boost Detection

| Attribute | Detail |
|-----------|--------|
| Feature | Track when users boost or remove boosts from a chat |
| API updates | `ChatBoostUpdated`, `ChatBoostRemoved` update types |
| API classes | `ChatBoostSourcePremium` (from Premium subscription), `ChatBoostSourceGiftCode`, `ChatBoostSourceGiveaway` |
| Premium requirement | **Users need Premium (or won a giveaway)** to boost; bots just observe |
| Added | Bot API 7.0 |

#### 8.2 Querying User Boosts

| Attribute | Detail |
|-----------|--------|
| Feature | Get a user's active boosts for a specific chat |
| API method | `getUserChatBoosts(chat_id, user_id)` returns `UserChatBoosts` |
| Premium requirement | None for the API call; user needs Premium to have boosts |
| Use case | Grant premium bot features to users who boost the channel |

#### 8.3 Boost-Gated Features

| Attribute | Detail |
|-----------|--------|
| Feature | Bots can check boost status and provide differentiated access |
| Pattern | Call `getUserChatBoosts`, check `boosts` array length, unlock features for active boosters |
| Premium requirement | Users who boost with Premium subscriptions count; each Premium gift code boosts 4x |
| Business value | Incentivize channel boosts by unlocking bot features for boosters |

#### 8.4 Giveaways

| Attribute | Detail |
|-----------|--------|
| Feature | Read and process giveaway lifecycle events |
| API classes | `Giveaway`, `GiveawayCreated`, `GiveawayWinners`, `GiveawayCompleted` |
| Fields | `prize_star_count`, `is_star_giveaway`, `ChatBoostSourceGiveaway` |
| Premium requirement | None to observe; creating giveaways is a client-side action by channel admins |
| Added | Bot API 7.0 and extended in 7.10 |

---

### 9. Stories

Stories integration requires an active Business Connection (Premium user).

#### 9.1 Post Stories

| Attribute | Detail |
|-----------|--------|
| Feature | Bot posts a story on behalf of a managed business account |
| API method | `postStory(business_connection_id, content, active_period, ...)` |
| Premium requirement | **Business account owner must have Premium** |
| Added | Bot API 8.x |

#### 9.2 Edit and Delete Stories

| Attribute | Detail |
|-----------|--------|
| Feature | Edit or delete stories posted by the bot |
| API methods | `editStory(business_connection_id, story_id, ...)`, `deleteStory(business_connection_id, story_id)` |
| Premium requirement | Active business connection (Premium) |

#### 9.3 Repost Stories

| Attribute | Detail |
|-----------|--------|
| Feature | Repost a story across multiple managed business accounts |
| API method | `repostStory(business_connection_id, from_chat_id, story_id)` |
| Premium requirement | Active business connections on source and destination |

#### 9.4 Story Interactive Areas

| Attribute | Detail |
|-----------|--------|
| Feature | Add interactive elements to stories: location, reaction, link, weather, unique gift |
| API classes | `StoryArea`, `StoryAreaPosition`, `StoryAreaTypeLocation`, `StoryAreaTypeSuggestedReaction`, `StoryAreaTypeLink`, `StoryAreaTypeWeather`, `StoryAreaTypeUniqueGift` |
| Premium requirement | Active business connection |

#### 9.5 Story Replies (Bot receives)

| Attribute | Detail |
|-----------|--------|
| Feature | Bots can detect when a user replies to a story |
| API field | `reply_to_story` in Message, `Story` class with `chat` and `id` fields |
| Premium requirement | None to receive; user needs Premium to post stories themselves |
| Added | Bot API 7.1 |

---

### 10. Message Effects

#### 10.1 Sending Messages with Visual Effects

| Attribute | Detail |
|-----------|--------|
| Feature | Attach a full-screen animated visual effect to a message (e.g., fireworks, confetti, hearts) |
| API parameter | `message_effect_id` (string) - added to `sendMessage`, `sendPhoto`, `sendVideo`, `sendAnimation`, `sendAudio`, `sendDocument`, `sendSticker`, `sendVideoNote`, `sendVoice`, `sendLocation`, `sendVenue`, `sendContact`, `sendPoll`, `sendDice`, `sendInvoice`, `sendGame`, `sendMediaGroup` |
| Field on received message | `Message.effect_id` |
| Premium requirement | **Some effects require Premium** (indicated by `premium_required` flag); others are free for all |
| Scope | **Private chats only** - effects do not work in groups or channels |
| Added | Bot API 7.4 (May 2024) |
| Graceful degradation | Do not pass `message_effect_id` in group/channel contexts; check if user is in private chat first |

#### 10.2 Known Free Effect IDs

These IDs are community-verified via MTProto `messages.getAvailableEffects`:

| Emoji | Effect | Effect ID |
|-------|--------|-----------|
| ❤️ | Hearts | `5159385139981059251` |
| 👍 | Like / Thumbs Up | `5107584321108051014` |
| 💩 | Turd | `5046589136895476101` |
| 👎 | Dislike | `5104858069142078462` |
| 🔥 | Flame / Fire | `5070445174516318631` |
| 🎉 | Confetti | `5066970843586925436` |

**Note:** Telegram does not publish an official effect ID list. IDs can also be captured from `MessageReactionUpdated` updates when users send effects. Premium-required effects (`premium_required: true` in MTProto schema) are available to bots with Fragment usernames.

#### 10.3 Getting Effect IDs Programmatically

To get the full list including premium effects, use the MTProto API (not Bot API):
- Method: `messages.getAvailableEffects()`
- Returns: `messages.availableEffects` with `Vector<AvailableEffect>`
- Each effect has: `id`, `emoticon`, `premium_required` flag, `effect_sticker_id`

---

### 11. Inline Query Enhancements

#### 11.1 Chat Type Awareness

| Attribute | Detail |
|-----------|--------|
| Feature | Know what type of chat the user is switching inline query from |
| API field | `InlineQuery.chat_type` - values: `sender`, `private`, `group`, `supergroup`, `channel` |
| Premium requirement | None |
| Added | Bot API 7.0 |
| Use case | Skip effects that only work in private chats when `chat_type != "private"` |

#### 11.2 Targeted Inline Mode Switching

| Attribute | Detail |
|-----------|--------|
| Feature | Switch to inline mode in specific chat types only |
| API class | `SwitchInlineQueryChosenChat` with `allow_user_chats`, `allow_bot_chats`, `allow_group_chats`, `allow_channel_chats` |
| Premium requirement | None |
| Added | Bot API 7.0 |

#### 11.3 Prepared Inline Messages (Mini App)

| Attribute | Detail |
|-----------|--------|
| Feature | Pre-build inline messages from Mini App for user to share to any chat |
| API method | `savePreparedInlineMessage(user_id, result, allow_user_chats, allow_bot_chats, allow_group_chats, allow_channel_chats)` |
| Premium requirement | None |
| Added | Bot API 8.0 |

#### 11.4 No Direct Premium Inline Enhancements

There are no Bot API inline query features that are specifically gated behind user Premium status. The chat_type field helps bots route behavior, and SwitchInlineQueryChosenChat controls targeting - both work for all bots.

---

### 12. Folders and Topics

#### 12.1 Forum Topics (Topic Threads in Groups)

| Attribute | Detail |
|-----------|--------|
| Feature | Bots can create, close, reopen, edit, and delete forum topics in supergroups |
| API methods | `createForumTopic`, `editForumTopic`, `closeForumTopic`, `reopenForumTopic`, `deleteForumTopic`, `unpinAllForumTopicMessages` |
| API parameter | `message_thread_id` in send methods to post into a topic |
| Premium requirement | None - forum topics are a group admin feature, not Premium-gated |

#### 12.2 Private Chat Topics (1-on-1 Topics)

| Attribute | Detail |
|-----------|--------|
| Feature | Bots can create topics in 1-on-1 (private) chats |
| Premium requirement | None for the API; BotFather allows bot owners to control topic creation |
| Notes | Relatively new feature; behavior controlled via BotFather Mini App settings |

#### 12.3 Chat Folders

Telegram Premium unlocks unlimited chat folders for users (non-Premium is limited). This is entirely a client-side feature - bots have no API access to a user's folder organization.

---

### 13. Additional Premium-Adjacent Features

#### 13.1 Gifting Telegram Premium Subscriptions

| Attribute | Detail |
|-----------|--------|
| Feature | Bot programmatically gifts a Telegram Premium subscription to a user |
| API method | `giftPremiumSubscription(user_id, star_count, month_count, text, text_parse_mode, text_entities)` |
| Premium requirement | Bot must have enough Telegram Stars to pay; no Premium required for bot owner |
| Added | Bot API 9.x (2025) |
| Use case | Reward users with Premium as part of a bot service |

#### 13.2 Emoji Status Management

| Attribute | Detail |
|-----------|--------|
| Feature | Bot sets a user's emoji status (the emoji shown next to their name) |
| API method | `setUserEmojiStatus(user_id, emoji_status, expiration_date)` |
| Mini App | `setEmojiStatus()` (user confirms via dialog), `requestEmojiStatusAccess()` |
| Premium requirement | **User needs Premium** to have an emoji status; bot needs user's permission |
| Added | Bot API 8.0 |
| Permission | User must authorize via `requestEmojiStatusAccess()` in Mini App first |

#### 13.3 Detecting Premium Users

| Attribute | Detail |
|-----------|--------|
| Feature | Identify if a user is a Telegram Premium subscriber |
| API field | `User.is_premium` (boolean, True if Premium) |
| Premium requirement | None to read; just present on all User objects |
| Use case | Gate bot features, skip Premium upsells, customize experience |

#### 13.4 Paid Broadcasts (Allow Paid Broadcast)

| Attribute | Detail |
|-----------|--------|
| Feature | Send messages to large audiences at higher rate (up to 1,000/sec vs 30/sec) |
| API parameter | `allow_paid_broadcast` in all send methods |
| Cost | 0.1 Stars per message over the free 30/sec limit; requires 10,000 Stars minimum balance |
| Premium requirement | None for the API; bot needs Stars balance |
| Added | Bot API 7.11 |

#### 13.5 Subscription-Based Chat Access

| Attribute | Detail |
|-----------|--------|
| Feature | Create invite links that require recurring Star payment |
| API method | `createChatSubscriptionInviteLink(chat_id, name, subscription_period, subscription_price)` |
| Premium requirement | None for creating; users pay Stars (not Premium) |
| Added | Bot API 7.9 |

#### 13.6 Paid Media

| Attribute | Detail |
|-----------|--------|
| Feature | Send media that requires Stars payment to view |
| API method | `sendPaidMedia(chat_id, star_count, media, business_connection_id, ...)` |
| Premium requirement | None for bot; users pay Stars |
| Added | Bot API 7.6 |
| Business | `business_connection_id` allows sending on behalf of Premium business account |

---

## Quick Reference: Premium Requirement Matrix

| Feature | Requires Bot Owner Premium | Requires User Premium | Works Without Any Premium |
|---------|---------------------------|----------------------|--------------------------|
| Custom emoji in messages | YES (or Fragment username) | No | No |
| Premium message effects | Fragment username | No | Free effects only |
| Free message effects (6 IDs) | No | No | YES |
| Standard blockquotes | No | No | YES |
| Expandable blockquotes | No | No | YES |
| Spoilers | No | No | YES |
| Standard emoji reactions | No | No | YES |
| Custom emoji reactions | Conditional (if already added by premium user) | Conditional | Partially |
| Forum topics | No | No | YES |
| Business bot connection | No | YES (they must have Premium) | No |
| Stories via business | No | YES | No |
| Large file (up to 4 GB) receive | No | YES (to send 4 GB) | Files <=2 GB |
| Boost detection | No | YES (to boost) | Can detect boosts |
| Emoji status set | No | YES (to have status) | Can read status |
| Gift Premium subscription | No (needs Stars) | No | YES (if Stars available) |
| Animated profile photos | No | YES (to set them) | Can read them |
| Paid reactions | No | No (Stars, not Premium) | Can observe |
| Inline chat_type field | No | No | YES |
| Forum topics | No | No | YES |

---

## Implementation Patterns

### Pattern 1: Custom Emoji with Fallback

```python
async def send_with_custom_emoji(bot, chat_id, custom_emoji_id, fallback_emoji):
    try:
        await bot.send_message(
            chat_id=chat_id,
            text="Hello!",
            entities=[MessageEntity(
                type="custom_emoji",
                offset=0,
                length=5,
                custom_emoji_id=custom_emoji_id
            )]
        )
    except TelegramError:
        # Bot owner does not have Premium - use standard emoji
        await bot.send_message(chat_id=chat_id, text=f"Hello! {fallback_emoji}")
```

### Pattern 2: Message Effect in Private Chat Only

```python
FREE_EFFECTS = {
    "hearts": "5159385139981059251",
    "confetti": "5066970843586925436",
    "fire": "5070445174516318631",
    "thumbsup": "5107584321108051014",
    "thumbsdown": "5104858069142078462",
    "poop": "5046589136895476101",
}

async def send_with_effect(bot, chat_id, text, effect_key, chat_type="private"):
    kwargs = {"chat_id": chat_id, "text": text}
    # Effects only work in private chats
    if chat_type == "private" and effect_key in FREE_EFFECTS:
        kwargs["message_effect_id"] = FREE_EFFECTS[effect_key]
    await bot.send_message(**kwargs)
```

### Pattern 3: Business Bot with Premium Detection

```python
async def handle_update(update, context):
    user = update.effective_user

    # Detect Premium user
    if user and user.is_premium:
        # Offer Premium-enhanced experience
        await offer_premium_features(update, context)

    # Handle business message
    if update.business_message:
        business_id = update.business_message.business_connection_id
        # Respond on behalf of business owner
        await context.bot.send_message(
            chat_id=update.business_message.chat.id,
            text="Response from business",
            business_connection_id=business_id
        )
```

### Pattern 4: Boost-Gated Features

```python
async def check_and_grant_boost_perks(bot, chat_id, user_id):
    boosts = await bot.get_user_chat_boosts(chat_id=chat_id, user_id=user_id)
    active_boosts = len(boosts.boosts)

    if active_boosts >= 1:
        # Grant basic boost perks
        return "BOOSTER"
    elif active_boosts >= 3:
        # Grant premium boost perks
        return "VIP_BOOSTER"
    return "STANDARD"
```

### Pattern 5: Custom Emoji Reactions with Fallback

```python
async def react_to_message(bot, chat_id, message_id, custom_emoji_id=None):
    if custom_emoji_id:
        try:
            await bot.set_message_reaction(
                chat_id=chat_id,
                message_id=message_id,
                reaction=[ReactionTypeCustomEmoji(custom_emoji_id=custom_emoji_id)]
            )
            return
        except TelegramError:
            pass  # Fall through to standard reaction

    # Fallback: standard emoji reaction
    await bot.set_message_reaction(
        chat_id=chat_id,
        message_id=message_id,
        reaction=[ReactionTypeEmoji(emoji="👍")]
    )
```

---

## API Version History (Premium-Relevant)

| Bot API Version | Date | Key Premium Feature |
|----------------|------|---------------------|
| 7.0 | Dec 2023 | Reactions (`setMessageReaction`), blockquotes, chat boosts, giveaways |
| 7.1 | Feb 2024 | Story reply detection (`reply_to_story`) |
| 7.2 | Mar 2024 | Business connection (`BusinessConnection`, business message updates) |
| 7.3 | May 2024 | Business message types, chat background classes |
| 7.4 | May 2024 | Message effects (`message_effect_id`), Telegram Stars payments |
| 7.5 | Jun 2024 | Star transactions, business message editing |
| 7.6 | Jul 2024 | Paid media (`sendPaidMedia`) |
| 7.8 | Jul 2024 | Story sharing from Mini Apps (`shareToStory`) |
| 7.9 | Aug 2024 | Paid media in any chat, subscription invite links, paid reactions |
| 7.11 | Oct 2024 | Paid broadcast (`allow_paid_broadcast`) |
| 8.0 | Nov 2024 | Star subscriptions, emoji status (`setUserEmojiStatus`), gifts |
| 8.1 | Dec 2024 | Affiliate programs |
| 8.2 | Jan 2025 | Third-party verification, gift upgrades |
| 8.3 | Feb 2025 | Stories (postStory/editStory/deleteStory), channel gifts |
| 9.x | 2025 | `giftPremiumSubscription`, premium subscription gifting via bot |

---

## Sources

- [Telegram Bot API](https://core.telegram.org/bots/api)
- [Bot API Changelog](https://core.telegram.org/bots/api-changelog)
- [Telegram Business API](https://core.telegram.org/api/business)
- [Message Reactions](https://core.telegram.org/api/reactions)
- [Custom Emoji](https://core.telegram.org/api/custom-emoji)
- [Animated Message Effects](https://core.telegram.org/api/effects)
- [Telegram Blog: Custom Emoji](https://telegram.org/blog/custom-emoji)
- [Telegram Blog: Message Effects](https://telegram.org/blog/message-effects-and-more)
- [Telegram Blog: Introducing Telegram Business](https://telegram.org/blog/telegram-business)
- [availableEffect Constructor](https://core.telegram.org/constructor/availableEffect)
- [messageEntityCustomEmoji Constructor](https://core.telegram.org/constructor/messageEntityCustomEmoji)
