/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use log::info;

use crate::consts::TRASHED_NOTE_EXPIRY_DAYS;
use crate::context::Context;

pub(crate) fn run(ctx: &Context) -> Result<()> {
    let cutoff = Utc::now() - Duration::from_secs(TRASHED_NOTE_EXPIRY_DAYS as u64 * 24 * 60 * 60);
    let result = ctx.database.execute(
        "DELETE FROM notes WHERE is_trashed = 1 AND updated_at <= ?",
        cutoff,
    )?;
    let count = result.affected_rows;
    if count > 0 {
        info!("Cleared {count} trashed note(s) older than {TRASHED_NOTE_EXPIRY_DAYS} days");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::DatabaseHelpers;
    use crate::models::Note;
    use crate::test_utils::create_test_user_with_session;

    #[test]
    fn cleanup_uses_millisecond_timestamps() {
        let ctx = Context::with_test_database().unwrap();
        let (user, _) = create_test_user_with_session(&ctx);
        let mut ids = Vec::new();
        for (age_days, is_trashed) in [(40, true), (10, true), (40, false)] {
            let note = Note {
                user_id: user.id,
                body: "cleanup test".into(),
                is_trashed,
                updated_at: Utc::now() - Duration::from_secs(age_days * 24 * 60 * 60),
                ..Default::default()
            };
            ids.push(note.id);
            ctx.database.insert_note(note).unwrap();
        }
        run(&ctx).unwrap();
        for (id, expected) in ids.into_iter().zip([0, 1, 1]) {
            assert_eq!(
                ctx.database
                    .query_some::<i64>("SELECT COUNT(*) FROM notes WHERE id = ?", id)
                    .unwrap(),
                expected
            );
        }
    }
}
