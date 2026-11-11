//! Polls: reading a poll's options and tallying the votes stored for it.

use sha2::{Digest, Sha256};
use whatsapp_rust::buffa::Message as _;

use crate::content;
use crate::store::{Result, Store, Tx};
use crate::wa;

/// One option of a poll with its current tally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PollOption {
    pub name: String,
    pub votes: usize,
    /// Whether this account voted for it.
    pub mine: bool,
}

/// A poll as the conversation shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PollView {
    pub question: String,
    pub options: Vec<PollOption>,
    /// How many options one person may pick (0 means any number).
    pub selectable: u32,
    pub voters: usize,
}

/// The identifier a vote uses for an option: the SHA-256 of its name, in hex.
pub fn option_hash(name: &str) -> String {
    hex(&Sha256::digest(name.as_bytes()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The poll a stored record carries, if it is one.
fn creation(record: &wa::WebMessageInfo) -> Option<&wa::message::PollCreationMessage> {
    let message = record.message.as_option()?;
    let (base, _, _) = content::unwrap(message);
    [
        &base.poll_creation_message,
        &base.poll_creation_message_v2,
        &base.poll_creation_message_v3,
        &base.poll_creation_message_v5,
        &base.poll_creation_message_v6,
    ]
    .into_iter()
    .find_map(|field| field.as_option())
}

/// The secret a vote on this poll must be encrypted with.
pub fn secret(record: &wa::WebMessageInfo) -> Option<Vec<u8>> {
    let from_context = record
        .message
        .as_option()
        .and_then(|m| m.message_context_info.as_option())
        .and_then(|ctx| ctx.message_secret.clone())
        .map(|secret| secret.to_vec());
    from_context
        .or_else(|| record.message_secret.clone().map(|secret| secret.to_vec()))
        .filter(|secret| !secret.is_empty())
}

/// Builds the view of a poll from its record and the votes stored for it.
/// `votes` is one entry per voter: who, and the option hashes they picked.
pub fn view(record: &wa::WebMessageInfo, votes: &[(String, Vec<String>)]) -> Option<PollView> {
    let poll = creation(record)?;
    let mut options: Vec<PollOption> = poll
        .options
        .iter()
        .filter_map(|option| option.option_name.clone())
        .map(|name| PollOption {
            name,
            votes: 0,
            mine: false,
        })
        .collect();
    let hashes: Vec<String> = options.iter().map(|o| option_hash(&o.name)).collect();
    let mut voters = 0;
    for (voter, picked) in votes {
        if picked.is_empty() {
            continue;
        }
        voters += 1;
        for (index, hash) in hashes.iter().enumerate() {
            if picked.contains(hash) {
                options[index].votes += 1;
                options[index].mine |= voter == "me";
            }
        }
    }
    Some(PollView {
        question: poll.name.clone().unwrap_or_default(),
        options,
        selectable: poll.selectable_options_count.unwrap_or(0),
        voters,
    })
}

impl Tx<'_> {
    /// Records one person's current picks on a poll (an empty list retracts the vote).
    pub fn put_poll_vote(&self, chat: &str, poll_id: &str, voter: &str, hashes: &[String], ts: i64) -> Result<()> {
        let chat = self.canonical(chat)?;
        let voter = if voter == "me" {
            voter.to_string()
        } else {
            self.canonical(voter)?
        };
        self.conn.execute(
            "INSERT INTO poll_votes (chat, poll_id, voter, options, ts) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (chat, poll_id, voter) DO UPDATE
             SET options = excluded.options, ts = excluded.ts WHERE excluded.ts >= ts",
            rusqlite::params![chat, poll_id, voter, hashes.join(","), ts],
        )?;
        Ok(())
    }

    /// Stores the votes a history record carries for its poll.
    pub fn put_poll_votes_of(&self, chat: &str, poll_id: &str, record: &wa::WebMessageInfo) -> Result<()> {
        for update in &record.poll_updates {
            let Some(key) = update.poll_update_message_key.as_option() else {
                continue;
            };
            let voter = if key.from_me.unwrap_or(false) {
                "me".to_string()
            } else {
                key.participant
                    .clone()
                    .filter(|p| !p.is_empty())
                    .or_else(|| key.remote_jid.clone())
                    .unwrap_or_default()
            };
            if voter.is_empty() {
                continue;
            }
            let hashes: Vec<String> = update
                .vote
                .as_option()
                .map(|vote| vote.selected_options.iter().map(|option| hex(option)).collect())
                .unwrap_or_default();
            self.put_poll_vote(chat, poll_id, &voter, &hashes, update.sender_timestamp_ms.unwrap_or(0) / 1000)?;
        }
        Ok(())
    }
}

impl Store {
    /// Everyone's current picks on a poll: (voter, option hashes).
    pub fn poll_votes(&self, chat: &str, poll_id: &str) -> Result<Vec<(String, Vec<String>)>> {
        self.read(|conn| {
            let mut stmt =
                conn.prepare_cached("SELECT voter, options FROM poll_votes WHERE chat = ?1 AND poll_id = ?2")?;
            let rows = stmt.query_map([chat, poll_id], |row| {
                let voter: String = row.get(0)?;
                let options: String = row.get(1)?;
                Ok((
                    voter,
                    options
                        .split(',')
                        .filter(|hash| !hash.is_empty())
                        .map(str::to_string)
                        .collect(),
                ))
            })?;
            rows.collect()
        })
    }

    /// The poll view of a stored message, if it is a poll.
    pub fn poll(&self, chat: &str, id: &str, proto: &[u8]) -> Option<PollView> {
        let record = wa::WebMessageInfo::decode_from_slice(proto).ok()?;
        let votes = self.poll_votes(chat, id).unwrap_or_default();
        view(&record, &votes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::buffa::MessageField;

    fn poll_record(options: &[&str]) -> wa::WebMessageInfo {
        let mut poll = wa::message::PollCreationMessage::default();
        poll.name = Some("Lunch?".to_string());
        poll.selectable_options_count = Some(1);
        for name in options {
            let mut option = wa::message::poll_creation_message::Option::default();
            option.option_name = Some(name.to_string());
            poll.options.push(option);
        }
        let mut message = wa::Message::default();
        message.poll_creation_message_v3 = MessageField::some(poll);
        let mut record = wa::WebMessageInfo::default();
        record.message = MessageField::some(message);
        record
    }

    #[test]
    fn tallies_votes_by_option_hash() {
        let record = poll_record(&["Pizza", "Soup"]);
        let votes = vec![
            ("me".to_string(), vec![option_hash("Pizza")]),
            ("a@s.whatsapp.net".to_string(), vec![option_hash("Pizza")]),
            ("b@s.whatsapp.net".to_string(), vec![option_hash("Soup")]),
            // A retracted vote counts for nothing.
            ("c@s.whatsapp.net".to_string(), Vec::new()),
        ];
        let view = view(&record, &votes).expect("is a poll");
        assert_eq!(view.question, "Lunch?");
        assert_eq!(view.voters, 3);
        assert_eq!(
            view.options,
            vec![
                PollOption {
                    name: "Pizza".into(),
                    votes: 2,
                    mine: true
                },
                PollOption {
                    name: "Soup".into(),
                    votes: 1,
                    mine: false
                },
            ]
        );
    }

    #[test]
    fn option_hash_is_sha256_hex() {
        assert_eq!(
            option_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn non_polls_have_no_view() {
        assert!(view(&wa::WebMessageInfo::default(), &[]).is_none());
    }
}
