//! whatsapp-gpui: a native Windows WhatsApp client.
//!
//! - [`store`] is the local database of chats, messages and contacts.
//! - [`content`] turns protocol messages into what the interface shows.
//! - [`import`] loads history-sync data into the store.
//! - [`paths`] says where everything lives on disk.

pub mod avatars;
pub mod backend;
pub mod buffa_decode;
pub mod content;
pub mod events;
pub mod import;
pub mod media_ops;
pub mod ingest;
pub mod model;
pub mod paths;
pub mod richtext;
pub mod store;
pub mod system;
pub mod ui;

pub use whatsapp_rust::waproto::whatsapp as wa;
