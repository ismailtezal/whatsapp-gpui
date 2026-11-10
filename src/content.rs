//! Turns protocol messages into what the interface shows: a kind, a text, and the
//! reply / forward / mention details that sit around the content.

use crate::model::MsgKind;
use crate::wa;

/// What a displayable message boils down to.
#[derive(Clone, Debug)]
pub struct Digest {
    pub kind: MsgKind,
    /// Body, caption, file name, poll question: whatever best stands for the message as text.
    pub text: Option<String>,
    pub quoted_id: Option<String>,
    pub quoted_sender: Option<String>,
    pub forwarded: bool,
    pub view_once: bool,
    pub edited: bool,
    pub mentions: Vec<String>,
    /// Disappearing-message timer carried by the message, in seconds.
    pub expiration: Option<u32>,
}

impl Digest {
    fn new(kind: MsgKind, text: Option<String>) -> Self {
        Digest {
            kind,
            text,
            quoted_id: None,
            quoted_sender: None,
            forwarded: false,
            view_once: false,
            edited: false,
            mentions: Vec::new(),
            expiration: None,
        }
    }
}

fn non_empty(text: &Option<String>) -> Option<String> {
    text.as_ref().filter(|t| !t.is_empty()).cloned()
}

/// Peels wrappers (disappearing, view-once, edited, sent-from-my-other-device, …) until the
/// message that carries the content. Also reports whether a view-once or edit wrapper was seen.
pub fn unwrap(message: &wa::Message) -> (&wa::Message, bool, bool) {
    let mut current = message;
    let mut view_once = false;
    let mut edited = false;
    // Wrappers nest at most a few levels; the bound only guards against a malformed loop.
    for _ in 0..8 {
        let mut next: Option<&wa::Message> = None;
        if let Some(inner) = current
            .device_sent_message
            .as_option()
            .and_then(|w| w.message.as_option())
        {
            next = Some(inner);
        }
        macro_rules! future_proof {
            ($field:ident) => {
                if next.is_none()
                    && let Some(inner) = current
                        .$field
                        .as_option()
                        .and_then(|w| w.message.as_option())
                {
                    next = Some(inner);
                }
            };
            ($field:ident, $flag:ident) => {
                if next.is_none()
                    && let Some(inner) = current
                        .$field
                        .as_option()
                        .and_then(|w| w.message.as_option())
                {
                    $flag = true;
                    next = Some(inner);
                }
            };
        }
        future_proof!(ephemeral_message);
        future_proof!(view_once_message, view_once);
        future_proof!(view_once_message_v2, view_once);
        future_proof!(view_once_message_v2_extension, view_once);
        future_proof!(document_with_caption_message);
        future_proof!(edited_message, edited);
        future_proof!(group_mentioned_message);
        future_proof!(bot_invoke_message);
        future_proof!(lottie_sticker_message);
        future_proof!(event_cover_image);
        future_proof!(status_mention_message);
        future_proof!(poll_creation_option_image_message);
        future_proof!(associated_child_message);
        future_proof!(group_status_mention_message);
        future_proof!(poll_creation_message_v4);
        future_proof!(status_add_yours);
        future_proof!(group_status_message);
        future_proof!(limit_sharing_message);
        future_proof!(bot_task_message);
        future_proof!(question_message);
        future_proof!(group_status_message_v2);
        future_proof!(bot_forwarded_message);
        future_proof!(spoiler_message);
        match next {
            Some(inner) => current = inner,
            None => break,
        }
    }
    (current, view_once, edited)
}

/// The reply / forward / mention block of whichever content the message carries.
pub fn context_info(base: &wa::Message) -> Option<&wa::ContextInfo> {
    macro_rules! ctx {
        ($($field:ident),* $(,)?) => {
            $(
                if let Some(ctx) = base
                    .$field
                    .as_option()
                    .and_then(|m| m.context_info.as_option())
                {
                    return Some(ctx);
                }
            )*
        };
    }
    ctx!(
        extended_text_message,
        image_message,
        video_message,
        audio_message,
        document_message,
        sticker_message,
        contact_message,
        contacts_array_message,
        location_message,
        live_location_message,
        poll_creation_message,
        poll_creation_message_v2,
        poll_creation_message_v3,
        event_message,
        group_invite_message,
        ptv_message,
        album_message,
        buttons_message,
        list_message,
        interactive_message,
        template_message,
        product_message,
        order_message,
    );
    None
}

/// Classifies a message. `None` means the message is an action on other messages or on the
/// chat (reaction, delete, edit, key exchange, …), not something to show as a bubble.
pub fn digest(message: &wa::Message) -> Option<Digest> {
    let (base, view_once, edited) = unwrap(message);
    let mut digest = classify(base)?;
    digest.view_once = view_once || inline_view_once(base);
    digest.edited = edited;
    if let Some(ctx) = context_info(base) {
        digest.quoted_id = non_empty(&ctx.stanza_id);
        digest.quoted_sender = non_empty(&ctx.participant);
        digest.forwarded = ctx.is_forwarded.unwrap_or(false);
        digest.mentions = ctx.mentioned_jid.clone();
        digest.expiration = ctx.expiration.filter(|seconds| *seconds > 0);
    }
    Some(digest)
}

fn inline_view_once(base: &wa::Message) -> bool {
    base.image_message
        .as_option()
        .and_then(|m| m.view_once)
        .or_else(|| base.video_message.as_option().and_then(|m| m.view_once))
        .or_else(|| base.audio_message.as_option().and_then(|m| m.view_once))
        .unwrap_or(false)
}

fn classify(base: &wa::Message) -> Option<Digest> {
    use MsgKind::*;
    if let Some(text) = non_empty(&base.conversation) {
        return Some(Digest::new(Text, Some(text)));
    }
    if let Some(m) = base.extended_text_message.as_option() {
        return Some(Digest::new(Text, non_empty(&m.text)));
    }
    if let Some(m) = base.image_message.as_option() {
        return Some(Digest::new(Image, non_empty(&m.caption)));
    }
    if let Some(m) = base.video_message.as_option() {
        let kind = if m.gif_playback.unwrap_or(false) {
            Gif
        } else {
            Video
        };
        return Some(Digest::new(kind, non_empty(&m.caption)));
    }
    if base.ptv_message.is_set() {
        return Some(Digest::new(VideoNote, None));
    }
    if let Some(m) = base.audio_message.as_option() {
        let kind = if m.ptt.unwrap_or(false) { Voice } else { Audio };
        return Some(Digest::new(kind, None));
    }
    if let Some(m) = base.document_message.as_option() {
        let text = non_empty(&m.caption)
            .or_else(|| non_empty(&m.file_name))
            .or_else(|| non_empty(&m.title));
        return Some(Digest::new(Document, text));
    }
    if base.sticker_message.is_set() || base.lottie_sticker_message.is_set() {
        return Some(Digest::new(Sticker, None));
    }
    if let Some(m) = base.location_message.as_option() {
        let text = non_empty(&m.name).or_else(|| non_empty(&m.address));
        return Some(Digest::new(Location, text));
    }
    if let Some(m) = base.live_location_message.as_option() {
        return Some(Digest::new(LiveLocation, non_empty(&m.caption)));
    }
    if let Some(m) = base.contact_message.as_option() {
        return Some(Digest::new(Contact, non_empty(&m.display_name)));
    }
    if let Some(m) = base.contacts_array_message.as_option() {
        let text = non_empty(&m.display_name).or_else(|| {
            let names: Vec<&str> = m
                .contacts
                .iter()
                .filter_map(|c| c.display_name.as_deref())
                .collect();
            (!names.is_empty()).then(|| names.join(", "))
        });
        return Some(Digest::new(Contacts, text));
    }
    for poll in [
        &base.poll_creation_message,
        &base.poll_creation_message_v2,
        &base.poll_creation_message_v3,
        &base.poll_creation_message_v5,
        &base.poll_creation_message_v6,
    ] {
        if let Some(m) = poll.as_option() {
            return Some(Digest::new(Poll, non_empty(&m.name)));
        }
    }
    if let Some(m) = base.poll_result_snapshot_message.as_option() {
        return Some(Digest::new(PollResult, non_empty(&m.name)));
    }
    if let Some(m) = base.event_message.as_option() {
        return Some(Digest::new(Event, non_empty(&m.name)));
    }
    if let Some(m) = base.group_invite_message.as_option() {
        let text = non_empty(&m.group_name).or_else(|| non_empty(&m.caption));
        return Some(Digest::new(GroupInvite, text));
    }
    if base.newsletter_admin_invite_message.is_set()
        || base.newsletter_follower_invite_message_v2.is_set()
    {
        return Some(Digest::new(NewsletterInvite, None));
    }
    if base.call_log_messsage.is_set()
        || base.call.is_set()
        || base.bcall_message.is_set()
        || base.scheduled_call_creation_message.is_set()
    {
        return Some(Digest::new(Call, None));
    }
    if base.album_message.is_set() {
        return Some(Digest::new(Album, None));
    }
    if base.sticker_pack_message.is_set() {
        return Some(Digest::new(StickerPack, None));
    }
    if let Some(m) = base.buttons_message.as_option() {
        return Some(Digest::new(Interactive, non_empty(&m.content_text)));
    }
    if let Some(m) = base.list_message.as_option() {
        let text = non_empty(&m.description).or_else(|| non_empty(&m.title));
        return Some(Digest::new(Interactive, text));
    }
    if let Some(m) = base.interactive_message.as_option() {
        let text = m.body.as_option().and_then(|b| non_empty(&b.text));
        return Some(Digest::new(Interactive, text));
    }
    if let Some(m) = base.buttons_response_message.as_option() {
        // The only response variant carries the label of the button that was pressed.
        let text = m
            .response
            .as_ref()
            .map(|response| format!("{response:?}"))
            .and_then(|debug| debug.split('"').nth(1).map(str::to_string))
            .or_else(|| non_empty(&m.selected_button_id));
        return Some(Digest::new(Text, text));
    }
    if let Some(m) = base.list_response_message.as_option() {
        return Some(Digest::new(Text, non_empty(&m.title)));
    }
    if let Some(m) = base.template_button_reply_message.as_option() {
        return Some(Digest::new(Text, non_empty(&m.selected_display_text)));
    }
    if let Some(m) = base.template_message.as_option() {
        let text = m
            .hydrated_template
            .as_option()
            .and_then(|t| non_empty(&t.hydrated_content_text));
        return Some(Digest::new(Template, text));
    }
    if base.highly_structured_message.is_set() {
        return Some(Digest::new(Template, None));
    }
    if base.product_message.is_set() {
        return Some(Digest::new(Product, None));
    }
    if let Some(m) = base.order_message.as_option() {
        return Some(Digest::new(Order, non_empty(&m.message)));
    }
    if base.send_payment_message.is_set()
        || base.request_payment_message.is_set()
        || base.decline_payment_request_message.is_set()
        || base.cancel_payment_request_message.is_set()
        || base.payment_invite_message.is_set()
        || base.invoice_message.is_set()
    {
        return Some(Digest::new(Payment, None));
    }

    // Actions and transport plumbing: handled elsewhere, never shown as a bubble.
    if base.protocol_message.is_set()
        || base.reaction_message.is_set()
        || base.enc_reaction_message.is_set()
        || base.poll_update_message.is_set()
        || base.keep_in_chat_message.is_set()
        || base.pin_in_chat_message.is_set()
        || base.enc_event_response_message.is_set()
        || base.sender_key_distribution_message.is_set()
        || base.fast_ratchet_key_sender_key_distribution_message.is_set()
        || base.sticker_sync_rmr_message.is_set()
        || base.message_history_bundle.is_set()
        || base.message_history_notice.is_set()
        || base.poll_add_option_message.is_set()
        || base.group_root_key_share.is_set()
        || base.root_secret_distribute_message.is_set()
        || base.secret_encrypted_message.is_set()
        || base.enc_comment_message.is_set()
    {
        return None;
    }

    // A message carrying only bookkeeping (context info, nothing else) is not content.
    let mut bare = base.clone();
    bare.message_context_info = Default::default();
    if bare == wa::Message::default() {
        return None;
    }
    Some(Digest::new(Unsupported, None))
}

/// The one-line stand-in for a message in the chat list and in reply quotes.
pub fn preview_label(kind: MsgKind, text: Option<&str>) -> String {
    use MsgKind::*;
    let text = text.filter(|t| !t.is_empty());
    let with = |label: &str| match text {
        Some(text) => format!("{label}: {text}"),
        None => label.to_string(),
    };
    match kind {
        Text | System => text.unwrap_or_default().to_string(),
        Image => text.map(str::to_string).unwrap_or_else(|| "Photo".into()),
        Video => text.map(str::to_string).unwrap_or_else(|| "Video".into()),
        Gif => text.map(str::to_string).unwrap_or_else(|| "GIF".into()),
        Audio => "Audio".into(),
        Voice => "Voice message".into(),
        VideoNote => "Video note".into(),
        Document => text.map(str::to_string).unwrap_or_else(|| "Document".into()),
        Sticker => "Sticker".into(),
        Location => with("Location"),
        LiveLocation => with("Live location"),
        Contact => with("Contact"),
        Contacts => with("Contacts"),
        Poll => with("Poll"),
        PollResult => with("Poll results"),
        Event => with("Event"),
        GroupInvite => with("Group invite"),
        NewsletterInvite => "Channel invite".into(),
        Call => "Call".into(),
        Revoked => "This message was deleted".into(),
        Album => "Album".into(),
        StickerPack => "Sticker pack".into(),
        Interactive | Template => text.unwrap_or("Message").to_string(),
        Product => "Product".into(),
        Order => with("Order"),
        Payment => "Payment".into(),
        Unsupported => "Unsupported message".into(),
    }
}

/// Strips the device part of a JID (`123:4@s.whatsapp.net` becomes `123@s.whatsapp.net`).
pub fn bare_jid(jid: &str) -> String {
    match jid.split_once('@') {
        Some((user, server)) => {
            let user = user.split(':').next().unwrap_or(user);
            let user = user.split('.').next().unwrap_or(user);
            format!("{user}@{server}")
        }
        None => jid.to_string(),
    }
}
