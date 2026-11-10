//! Wording for chat notices: the centred grey lines such as "X added Y" or
//! "Messages are end-to-end encrypted".

use crate::wa;
use crate::wa::web_message_info::StubType;

/// Builds the text of a notice.
///
/// `name` turns a JID into a display name ("You" for the account itself). Returns `None`
/// for notice types that the official client does not show either.
pub fn notice_text(record: &wa::WebMessageInfo, name: &dyn Fn(&str) -> String) -> Option<String> {
    let stub = record.message_stub_type?;
    let params = &record.message_stub_parameters;
    let first = params.first().map(String::as_str).unwrap_or_default();

    // Who did it: the participant on the record (groups), else the chat peer.
    let key = record.key.as_option();
    let from_me = key.and_then(|k| k.from_me).unwrap_or(false);
    let actor_jid = record
        .participant
        .as_deref()
        .or_else(|| key.and_then(|k| k.participant.as_deref()))
        .filter(|p| !p.is_empty());
    let actor = if from_me {
        "You".to_string()
    } else {
        actor_jid.map(name).unwrap_or_else(|| "Someone".to_string())
    };
    let targets = || {
        let names: Vec<String> = params
            .iter()
            .filter(|p| p.contains('@'))
            .map(|p| name(p))
            .collect();
        join_names(&names)
    };
    // Whether the only target is the actor (joined / left by themselves).
    let self_action = actor_jid.is_some_and(|a| params.len() == 1 && params[0] == a);

    use StubType::*;
    let text = match stub {
        REVOKE | ADMIN_REVOKE => "This message was deleted".to_string(),
        CIPHERTEXT => "Waiting for this message. This may take a while.".to_string(),
        E2E_ENCRYPTED | E2E_ENCRYPTED_NOW => {
            "Messages and calls are end-to-end encrypted. Only people in this chat can read, \
             listen to, or share them."
                .to_string()
        }
        E2E_IDENTITY_CHANGED => format!("Your security code with {} changed.", targets_or(&targets(), &actor)),
        E2E_DEVICE_CHANGED => format!("{}'s list of devices changed.", targets_or(&targets(), &actor)),
        GROUP_CREATE => format!("{actor} created group \"{first}\""),
        GROUP_CHANGE_SUBJECT => format!("{actor} changed the group name to \"{first}\""),
        GROUP_CHANGE_ICON => {
            if first == "remove" {
                format!("{actor} removed the group icon")
            } else {
                format!("{actor} changed this group's icon")
            }
        }
        GROUP_CHANGE_INVITE_LINK => format!("{actor} reset this group's invite link"),
        GROUP_CHANGE_DESCRIPTION => format!("{actor} changed the group description"),
        GROUP_CHANGE_RESTRICT => {
            if first == "on" {
                format!("{actor} changed this group's settings to allow only admins to edit group info")
            } else {
                format!("{actor} changed this group's settings to allow all members to edit group info")
            }
        }
        GROUP_CHANGE_ANNOUNCE => {
            if first == "on" {
                format!("{actor} changed this group's settings to allow only admins to send messages")
            } else {
                format!("{actor} changed this group's settings to allow all members to send messages")
            }
        }
        GROUP_PARTICIPANT_ADD | GROUP_PARTICIPANT_ADD_REQUEST_JOIN | GROUP_PARTICIPANT_ACCEPT => {
            if self_action || actor_jid.is_none() {
                format!("{} joined", targets_or(&targets(), &actor))
            } else {
                format!("{actor} added {}", targets())
            }
        }
        GROUP_PARTICIPANT_LINKED_GROUP_JOIN | GROUP_PARTICIPANT_JOINED_GROUP_AND_PARENT_GROUP => {
            format!("{} joined from the community", targets_or(&targets(), &actor))
        }
        GROUP_PARTICIPANT_REMOVE => {
            if self_action || actor_jid.is_none() {
                format!("{} left", targets_or(&targets(), &actor))
            } else {
                format!("{actor} removed {}", targets())
            }
        }
        GROUP_PARTICIPANT_LEAVE => format!("{} left", targets_or(&targets(), &actor)),
        GROUP_PARTICIPANT_PROMOTE | COMMUNITY_PARTICIPANT_PROMOTE => {
            format!("{} {} now an admin", targets_or(&targets(), &actor), is_are(params.len()))
        }
        GROUP_PARTICIPANT_DEMOTE | COMMUNITY_PARTICIPANT_DEMOTE => {
            format!("{} {} no longer an admin", targets_or(&targets(), &actor), is_are(params.len()))
        }
        GROUP_PARTICIPANT_INVITE | GROUP_V4_ADD_INVITE_SENT => {
            format!("{actor} invited {}", targets())
        }
        GROUP_PARTICIPANT_CHANGE_NUMBER | INDIVIDUAL_CHANGE_NUMBER => {
            format!("{actor} changed their phone number to a new number.")
        }
        GROUP_DELETE => "This group has ended".to_string(),
        GROUP_ANNOUNCE_MODE_MESSAGE_BOUNCE => "Only admins can send messages to this group".to_string(),
        GROUP_MEMBERSHIP_JOIN_APPROVAL_MODE => {
            if first == "on" {
                format!("{actor} turned on admin approval to join this group")
            } else {
                format!("{actor} turned off admin approval to join this group")
            }
        }
        GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST | GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST_NON_ADMIN_ADD => {
            format!("{} requested to join", targets_or(&targets(), &actor))
        }
        GROUP_MEMBER_ADD_MODE => {
            if first == "all_member_add" {
                format!("{actor} changed this group's settings to allow all members to add others")
            } else {
                format!("{actor} changed this group's settings to allow only admins to add others")
            }
        }
        GROUP_CHANGE_NO_FREQUENTLY_FORWARDED => {
            format!("{actor} changed the setting for frequently forwarded messages")
        }
        CALL_MISSED_VOICE | SILENCED_UNKNOWN_CALLER_AUDIO => "Missed voice call".to_string(),
        CALL_MISSED_VIDEO | SILENCED_UNKNOWN_CALLER_VIDEO => "Missed video call".to_string(),
        CALL_MISSED_GROUP_VOICE => "Missed group voice call".to_string(),
        CALL_MISSED_GROUP_VIDEO => "Missed group video call".to_string(),
        CHANGE_EPHEMERAL_SETTING | DISAPPEARING_MODE => match first.parse::<u64>() {
            Ok(0) | Err(_) => format!("{actor} turned off disappearing messages."),
            Ok(seconds) => format!(
                "{actor} turned on disappearing messages. New messages will disappear from this \
                 chat {} after they're sent.",
                duration_words(seconds)
            ),
        },
        EPHEMERAL_KEEP_IN_CHAT => format!("{actor} kept a message"),
        VIEWED_ONCE => "Opened".to_string(),
        BLOCK_CONTACT => {
            if first == "true" {
                "You blocked this contact".to_string()
            } else {
                "You unblocked this contact".to_string()
            }
        }
        PINNED_MESSAGE_IN_CHAT => format!("{actor} pinned a message"),
        BROADCAST_CREATE => format!("{actor} created a broadcast list"),
        BROADCAST_ADD => format!("{} added to the list", targets()),
        BROADCAST_REMOVE => format!("{} removed from the list", targets()),
        COMMUNITY_CREATE => format!("{actor} created the community \"{first}\""),
        COMMUNITY_LINK_PARENT_GROUP
        | COMMUNITY_LINK_PARENT_GROUP_RICH
        | COMMUNITY_LINK_PARENT_GROUP_MEMBERSHIP_APPROVAL => {
            format!("This group was added to the community \"{first}\"")
        }
        COMMUNITY_LINK_SUB_GROUP | COMMUNITY_LINK_SIBLING_GROUP => {
            format!("{actor} added the group \"{first}\"")
        }
        COMMUNITY_UNLINK_PARENT_GROUP | INTEGRITY_UNLINK_PARENT_GROUP => {
            "This group was removed from the community".to_string()
        }
        COMMUNITY_UNLINK_SUB_GROUP | COMMUNITY_UNLINK_SIBLING_GROUP => {
            format!("{actor} removed the group \"{first}\"")
        }
        COMMUNITY_PARENT_GROUP_DELETED => "The community was deactivated".to_string(),
        COMMUNITY_PARENT_GROUP_SUBJECT_CHANGED => {
            format!("{actor} changed the community name to \"{first}\"")
        }
        COMMUNITY_CHANGE_DESCRIPTION => format!("{actor} changed the community description"),
        COMMUNITY_OWNER_UPDATED => format!("{} is now the community owner", targets_or(&targets(), &actor)),
        COMMUNITY_PARTICIPANT_ADD_RICH | CAG_INVITE_AUTO_ADD | CAG_INVITE_AUTO_JOINED => {
            format!("{} joined the community", targets_or(&targets(), &actor))
        }
        GENERAL_CHAT_CREATE => "The general chat was created".to_string(),
        EVENT_UPDATED => format!("{actor} updated the event"),
        EVENT_CANCELED => format!("{actor} cancelled the event"),
        SCHEDULED_CALL_START_MESSAGE => format!("{actor} started a scheduled call"),
        SCHEDULED_CALL_CANCEL => format!("{actor} cancelled a scheduled call"),
        CHANGE_USERNAME => format!("{actor} changed their username"),
        GROUP_DEACTIVATED => "This group is no longer available".to_string(),
        BIZ_INTRO_TOP | BIZ_INTRO_BOTTOM | BIZ_PRIVACY_MODE_INIT_FB | BIZ_PRIVACY_MODE_INIT_BSP => {
            "This business uses a secure service from Meta to manage this chat.".to_string()
        }
        BIZ_PRIVACY_MODE_TO_FB | BIZ_PRIVACY_MODE_TO_BSP | BIZ_TWO_TIER_MIGRATION_TOP
        | BIZ_TWO_TIER_MIGRATION_BOTTOM => {
            "This business now uses a secure service from Meta to manage this chat.".to_string()
        }
        BIZ_NAME_CHANGE => format!("This business changed its name to \"{first}\""),
        BIZ_MOVE_TO_CONSUMER_APP => "This business account is now a standard account.".to_string(),
        VERIFIED_HIGH | VERIFIED_INITIAL_HIGH | VERIFIED_TRANSITION_ANY_TO_HIGH => {
            "This is an official business account.".to_string()
        }
        OVERSIZED => "This message is too large to show".to_string(),
        FUTUREPROOF | PAYMENT_FUTUREPROOF => {
            "This message is not supported by this version of the app".to_string()
        }
        SUPPORT_SYSTEM_MESSAGE | GENERIC_NOTIFICATION | CHAT_PSA => first.to_string(),
        // Internal bookkeeping notices that the official client does not display.
        _ => return None,
    };
    (!text.trim().is_empty()).then_some(text)
}

fn targets_or(targets: &str, fallback: &str) -> String {
    if targets.is_empty() {
        fallback.to_string()
    } else {
        targets.to_string()
    }
}

fn is_are(count: usize) -> &'static str {
    if count > 1 { "are" } else { "is" }
}

fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

fn duration_words(seconds: u64) -> String {
    match seconds {
        86_400 => "24 hours".to_string(),
        604_800 => "7 days".to_string(),
        7_776_000 => "90 days".to_string(),
        s if s % 86_400 == 0 => format!("{} days", s / 86_400),
        s if s % 3_600 == 0 => format!("{} hours", s / 3_600),
        s => format!("{} minutes", (s / 60).max(1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_names_like_a_sentence() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_names(&names(&[])), "");
        assert_eq!(join_names(&names(&["A"])), "A");
        assert_eq!(join_names(&names(&["A", "B"])), "A and B");
        assert_eq!(join_names(&names(&["A", "B", "C"])), "A, B, and C");
    }

    #[test]
    fn words_common_disappearing_timers() {
        assert_eq!(duration_words(86_400), "24 hours");
        assert_eq!(duration_words(604_800), "7 days");
        assert_eq!(duration_words(7_776_000), "90 days");
    }
}
