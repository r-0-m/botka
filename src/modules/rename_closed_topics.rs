//! Prefixes closed topics with "\[×] " and removes the prefix when they are
//! reopened.

use std::sync::Arc;

use anyhow::Result;
use diesel::prelude::*;
use regex::Regex;
use teloxide::prelude::*;
use teloxide::types::MessageKind;

use crate::common::BotEnv;
use crate::db::{DbChatId, DbThreadId};

lazy_static::lazy_static! {
    static ref CLOSED_TOPIC_REGEX: Regex =
        Regex::new(r"^\[[×xXхХ]\] *").unwrap();
}

pub async fn inspect_message<'a>(bot: Bot, env: Arc<BotEnv>, msg: Message) {
    if let Err(e) = inspect_message_result(bot, env, msg).await {
        log::error!("Error handling message: {}", e);
    }
}

async fn inspect_message_result<'a>(
    bot: Bot,
    env: Arc<BotEnv>,
    msg: Message,
) -> Result<()> {
    let Some(thread_id) = msg.thread_id else { return Ok(()) };

    let closed = match msg.kind {
        MessageKind::ForumTopicClosed(_) => true,
        MessageKind::ForumTopicReopened(_) => false,
        _ => return Ok(()),
    };

    use crate::schema::tg_chat_topics::dsl as t;
    let Some(old_name) = t::tg_chat_topics
        .filter(t::chat_id.eq(DbChatId::from(msg.chat.id)))
        .filter(t::topic_id.eq(DbThreadId::from(thread_id)))
        .select(t::name)
        .first::<Option<String>>(&mut *env.conn())?
    else {
        return Ok(());
    };

    let new_name = if closed {
        format!("[×] {old_name}")
    } else {
        CLOSED_TOPIC_REGEX.replace(&old_name, "").to_string()
    };
    if new_name == old_name {
        return Ok(());
    }

    bot.edit_forum_topic(msg.chat.id, thread_id).name(&new_name).await?;

    let update_count = diesel::update(t::tg_chat_topics)
        .filter(t::chat_id.eq(DbChatId::from(msg.chat.id)))
        .filter(t::topic_id.eq(DbThreadId::from(thread_id)))
        .filter(t::name.eq(old_name)) // Optimistic locking
        .set(t::name.eq(new_name))
        .execute(&mut *env.conn())?;

    if update_count != 1 {
        return Err(anyhow::anyhow!(
            "Failed to update topic name: already updated?"
        ));
    }

    Ok(())
}
