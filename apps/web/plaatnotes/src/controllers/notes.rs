/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use anyhow::Result;
use bsql::{execute_args, preprocess_fts_query, query_args};
use chrono::Utc;
use from_derive::FromStruct;
use small_http::{Request, Response, Status};
use uuid::Uuid;
use validate::Validate;

use crate::api;
use crate::context::{Context, DatabaseHelpers};
use crate::controllers::{not_found, parse_body, parse_index_query, require_auth};
use crate::models::note::{
    FILTER_ARCHIVED, FILTER_NORMAL, FILTER_PINNED, FILTER_TRASHED, policies,
};
use crate::models::{IndexQuery, Note, User, UserRole};

// MARK: Handlers
pub(crate) fn notes_index(req: &Request, ctx: &Context) -> Result<Response> {
    notes_filtered(req, ctx, FILTER_NORMAL)
}

#[derive(Validate, FromStruct)]
#[from_struct(api::NoteCreateBody, only_from)]
pub(crate) struct NoteCreateBody {
    #[validate(length(min = 1))]
    pub(crate) title: Option<String>,
    #[validate(length(min = 1))]
    pub(crate) body: String,
    pub(crate) is_pinned: Option<bool>,
}

#[derive(Validate, FromStruct)]
#[from_struct(api::NoteAdminCreateBody, only_from)]
struct NoteAdminCreateBody {
    pub(crate) user_id: Uuid,
    #[validate(length(min = 1))]
    pub(crate) title: Option<String>,
    #[validate(length(min = 1))]
    pub(crate) body: String,
    pub(crate) is_pinned: Option<bool>,
}

pub(crate) fn notes_create(req: &Request, ctx: &Context) -> Result<Response> {
    let auth_user = require_auth!(ctx);

    if !policies::can_create(auth_user) {
        return Ok(Response::with_status(Status::Forbidden));
    }

    let body = parse_body!(req, api::NoteAdminCreateBody, NoteAdminCreateBody);

    // Verify the target user exists
    let target_user = match crate::controllers::users::get_user(body.user_id, ctx)? {
        Some(user) => user,
        None => return not_found(req, ctx),
    };

    let note = insert_note(
        ctx,
        target_user.id,
        body.title,
        body.body,
        body.is_pinned.unwrap_or(false),
    )?;

    Ok(Response::with_status(Status::Created).json(api::Note::from(note)))
}

pub(crate) fn notes_show(req: &Request, ctx: &Context) -> Result<Response> {
    let user = require_auth!(ctx);

    // Get note (admins can access any note, normal users only their own)
    let note_id = match parse_note_id(req) {
        Ok(id) => id,
        Err(_) => return Ok(Response::with_status(Status::BadRequest)),
    };
    let note = match fetch_note_for_user(note_id, ctx, user)? {
        Some(note) => note,
        None => return not_found(req, ctx),
    };

    // Check authorization
    if !policies::can_show(user, &note) {
        return Ok(Response::with_status(Status::Forbidden));
    }

    // Return note
    Ok(Response::with_json(api::Note::from(note)))
}

#[derive(Validate, FromStruct)]
#[from_struct(api::NoteUpdateBody, only_from)]
struct NoteUpdateBody {
    user_id: Option<Uuid>,
    #[validate(length(min = 1))]
    title: Option<String>,
    #[validate(length(min = 1))]
    body: String,
    is_pinned: bool,
    is_archived: bool,
    is_trashed: bool,
}

pub(crate) fn notes_update(req: &Request, ctx: &Context) -> Result<Response> {
    let user = require_auth!(ctx);

    // Get note (admins can access any note, normal users only their own)
    let note_id = match parse_note_id(req) {
        Ok(id) => id,
        Err(_) => return Ok(Response::with_status(Status::BadRequest)),
    };
    let mut note = match fetch_note_for_user(note_id, ctx, user)? {
        Some(note) => note,
        None => return not_found(req, ctx),
    };

    // Check authorization
    if !policies::can_update(user, &note) {
        return Ok(Response::with_status(Status::Forbidden));
    }

    let body = parse_body!(req, api::NoteUpdateBody, NoteUpdateBody);

    // Admins can reassign a note to a different user
    if let Some(new_user_id) = body.user_id {
        if user.role != UserRole::Admin {
            return Ok(Response::with_status(Status::Forbidden));
        }
        note.user_id = new_user_id;
    }

    // Update note
    let prev_is_archived = note.is_archived;
    let prev_is_trashed = note.is_trashed;
    note.title = body.title;
    note.body = body.body;
    note.is_pinned = body.is_pinned;
    note.is_archived = body.is_archived;
    note.is_trashed = body.is_trashed;
    note.updated_at = Utc::now();
    ctx.database.transaction(|db| -> Result<()> {
        execute_args!(
            db,
            "UPDATE notes SET user_id = :user_id, title = :title, body = :body, is_pinned = :is_pinned, is_archived = :is_archived, is_trashed = :is_trashed, updated_at = :updated_at WHERE id = :id",
            Args {
                user_id: note.user_id,
                title: note.title.clone(),
                body: note.body.clone(),
                is_pinned: note.is_pinned,
                is_archived: note.is_archived,
                is_trashed: note.is_trashed,
                updated_at: note.updated_at,
                id: note.id
            }
        )?;

        // When archiving or unarchiving, put the note first and shift all others
        if note.is_archived != prev_is_archived && !note.is_trashed {
            let filter = if note.is_archived {
                FILTER_ARCHIVED
            } else {
                FILTER_NORMAL
            };
            execute_args!(
                db,
                &format!(
                    "UPDATE notes SET position = position + 1 WHERE id != :id AND {filter} AND user_id = :user_id"
                ),
                Args {
                    id: note.id,
                    user_id: note.user_id
                }
            )?;
            execute_args!(
                db,
                "UPDATE notes SET position = 0 WHERE id = :id",
                Args { id: note.id }
            )?;
            note.position = 0;
        }

        // When trashing or untrashing, reset position in the destination category
        if note.is_trashed != prev_is_trashed {
            let filter = if note.is_trashed {
                FILTER_TRASHED
            } else if note.is_pinned {
                FILTER_PINNED
            } else if note.is_archived {
                FILTER_ARCHIVED
            } else {
                FILTER_NORMAL
            };
            execute_args!(
                db,
                &format!(
                    "UPDATE notes SET position = position + 1 WHERE id != :id AND {filter} AND user_id = :user_id"
                ),
                Args {
                    id: note.id,
                    user_id: note.user_id
                }
            )?;
            execute_args!(
                db,
                "UPDATE notes SET position = 0 WHERE id = :id",
                Args { id: note.id }
            )?;
            note.position = 0;
        }
        Ok(())
    })?;

    // Return updated note
    Ok(Response::with_json(api::Note::from(note)))
}

pub(crate) fn notes_delete(req: &Request, ctx: &Context) -> Result<Response> {
    let user = require_auth!(ctx);

    // Get note (admins can access any note, normal users only their own)
    let note_id = match parse_note_id(req) {
        Ok(id) => id,
        Err(_) => return Ok(Response::with_status(Status::BadRequest)),
    };
    let note = match fetch_note_for_user(note_id, ctx, user)? {
        Some(note) => note,
        None => return not_found(req, ctx),
    };

    // Check authorization
    if !policies::can_delete(user, &note) {
        return Ok(Response::with_status(Status::Forbidden));
    }

    // Delete note
    ctx.database
        .execute("DELETE FROM notes WHERE id = ?", note.id)?;

    // Success response
    Ok(Response::with_status(Status::NoContent))
}

// MARK: Utils
fn notes_filtered(req: &Request, ctx: &Context, filter: &str) -> Result<Response> {
    let user = require_auth!(ctx);

    if !policies::can_index(user) {
        return Ok(Response::with_status(Status::Forbidden));
    }

    let query = parse_index_query!(req);
    let (total, notes) = fetch_notes_page(ctx, filter, None, &query)?;

    Ok(Response::with_json(api::NoteIndexResponse {
        pagination: api::Pagination {
            page: query.page,
            limit: query.limit,
            total,
        },
        data: notes,
    }))
}

pub(crate) fn fetch_notes_page(
    ctx: &Context,
    filter: &str,
    user_id: Option<Uuid>,
    query: &IndexQuery,
) -> Result<(i64, Vec<api::Note>)> {
    let offset = (query.page - 1) * query.limit;
    if let Some(q) = query.query.as_deref().filter(|s| !s.is_empty()) {
        let fts_query = preprocess_fts_query(q);
        match user_id {
            None => {
                let total = ctx.database.query_some::<i64>(
                    &format!("SELECT COUNT(id) FROM notes WHERE {filter} AND id IN (SELECT id FROM notes_fts WHERE notes_fts MATCH ?)"),
                    fts_query.clone(),
                )?;
                let notes = query_args!(
                    Note, ctx.database,
                    format!("SELECT {} FROM notes WHERE {filter} AND id IN (SELECT id FROM notes_fts WHERE notes_fts MATCH :fts_query) ORDER BY position ASC, updated_at DESC LIMIT :limit OFFSET :offset", Note::columns()),
                    Args { fts_query: fts_query, limit: query.limit, offset: offset }
                )?.map(|r| r.map(Into::into)).collect::<Result<Vec<_>, _>>()?;
                Ok((total, notes))
            }
            Some(uid) => {
                let total = ctx.database.query_some::<i64>(
                    &format!("SELECT COUNT(id) FROM notes WHERE {filter} AND user_id = ? AND id IN (SELECT id FROM notes_fts WHERE notes_fts MATCH ?)"),
                    (uid, fts_query.clone()),
                )?;
                let notes = query_args!(
                    Note, ctx.database,
                    format!("SELECT {} FROM notes WHERE {filter} AND user_id = :user_id AND id IN (SELECT id FROM notes_fts WHERE notes_fts MATCH :fts_query) ORDER BY position ASC, updated_at DESC LIMIT :limit OFFSET :offset", Note::columns()),
                    Args { user_id: uid, fts_query: fts_query, limit: query.limit, offset: offset }
                )?.map(|r| r.map(Into::into)).collect::<Result<Vec<_>, _>>()?;
                Ok((total, notes))
            }
        }
    } else {
        match user_id {
            None => {
                let total = ctx.database.query_some::<i64>(
                    &format!("SELECT COUNT(id) FROM notes WHERE {filter}"),
                    (),
                )?;
                let notes = query_args!(
                    Note, ctx.database,
                    format!("SELECT {} FROM notes WHERE {filter} ORDER BY position ASC, updated_at DESC LIMIT :limit OFFSET :offset", Note::columns()),
                    Args { limit: query.limit, offset: offset }
                )?.map(|r| r.map(Into::into)).collect::<Result<Vec<_>, _>>()?;
                Ok((total, notes))
            }
            Some(uid) => {
                let total = ctx.database.query_some::<i64>(
                    &format!("SELECT COUNT(id) FROM notes WHERE {filter} AND user_id = ?"),
                    uid,
                )?;
                let notes = query_args!(
                    Note, ctx.database,
                    format!("SELECT {} FROM notes WHERE {filter} AND user_id = :user_id ORDER BY position ASC, updated_at DESC LIMIT :limit OFFSET :offset", Note::columns()),
                    Args { user_id: uid, limit: query.limit, offset: offset }
                )?.map(|r| r.map(Into::into)).collect::<Result<Vec<_>, _>>()?;
                Ok((total, notes))
            }
        }
    }
}

fn parse_note_id(req: &Request) -> Result<Uuid> {
    match req
        .params
        .get("note_id")
        .and_then(|id| id.parse::<Uuid>().ok())
    {
        Some(id) => Ok(id),
        None => anyhow::bail!("Invalid UUID"),
    }
}

fn fetch_note_for_user(note_id: Uuid, ctx: &Context, user: &User) -> Result<Option<Note>> {
    match user.role {
        UserRole::Admin => Ok(query_args!(
            Note,
            ctx.database,
            format!(
                "SELECT {} FROM notes WHERE id = :note_id LIMIT 1",
                Note::columns()
            ),
            Args { note_id: note_id }
        )?
        .next()
        .transpose()?),
        UserRole::Normal => Ok(query_args!(
            Note,
            ctx.database,
            format!(
                "SELECT {} FROM notes WHERE id = :note_id AND user_id = :user_id LIMIT 1",
                Note::columns()
            ),
            Args {
                note_id: note_id,
                user_id: user.id
            }
        )?
        .next()
        .transpose()?),
    }
}

pub(crate) fn notes_reorder_for(
    ctx: &Context,
    user: &User,
    ids: &[Uuid],
    filter: &str,
) -> Result<()> {
    ctx.database.transaction(|db| -> Result<()> {
        let provided_ids: Vec<Uuid> = ids.to_vec();

        // Fetch all note IDs in this category ordered by current position
        let all_ids: Vec<Uuid> = query_args!(
            Note,
            db,
            format!(
                "SELECT {} FROM notes WHERE {filter} AND user_id = :user_id ORDER BY position ASC, updated_at DESC",
                Note::columns()
            ),
            Args { user_id: user.id }
        )?
        .filter_map(|r| r.ok())
        .map(|n| n.id)
        .collect();

        // Filter provided IDs to only include notes that belong to this user/filter
        let provided_ids: Vec<Uuid> = provided_ids
            .into_iter()
            .filter(|id| all_ids.contains(id))
            .collect();

        // Notes not in the provided list follow in their existing relative order
        let rest_ids: Vec<Uuid> = all_ids
            .into_iter()
            .filter(|id| !provided_ids.contains(id))
            .collect();

        // Final sequence: provided notes first (in given order), then the rest
        for (position, note_id) in provided_ids.into_iter().chain(rest_ids).enumerate() {
            execute_args!(
                db,
                "UPDATE notes SET position = :position WHERE id = :note_id AND user_id = :user_id",
                Args {
                    position: position as i64,
                    note_id: note_id,
                    user_id: user.id,
                }
            )?;
        }
        Ok(())
    })
}

pub(crate) fn insert_note(
    ctx: &Context,
    user_id: Uuid,
    title: Option<String>,
    body: String,
    is_pinned: bool,
) -> Result<Note> {
    let note = Note {
        user_id,
        title,
        body,
        is_pinned,
        is_archived: false,
        is_trashed: false,
        ..Default::default()
    };

    // Shift all existing notes in the same category down so the new note appears first
    let position_filter = if note.is_pinned {
        FILTER_PINNED
    } else {
        FILTER_NORMAL
    };
    ctx.database.transaction(|db| -> Result<()> {
        execute_args!(
            db,
            &format!(
                "UPDATE notes SET position = position + 1 WHERE {position_filter} AND user_id = :user_id"
            ),
            Args { user_id: user_id }
        )?;

        db.insert_note(note.clone())?;
        Ok(())
    })?;
    Ok(note)
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;
    use crate::router;
    use crate::test_utils::{
        create_test_user_with_session, create_test_user_with_session_and_role, insert_test_note,
    };

    #[test]
    fn test_notes_index() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/notes", user.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        assert!(
            serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
                .unwrap()
                .data
                .is_empty()
        );

        insert_test_note(
            &ctx,
            user.id,
            Some("My First Note"),
            "This is my first note",
        );

        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/notes", user.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let notes = serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
            .unwrap()
            .data;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].body, "This is my first note");
    }

    #[test]
    fn test_notes_index_forbidden_for_normal_user() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token) = create_test_user_with_session(&ctx);

        let res = router.handle(
            Request::get("http://localhost/api/notes")
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Forbidden);
    }

    #[test]
    fn test_notes_index_excludes_pinned() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        insert_test_note(&ctx, user.id, Some("Regular Note"), "Regular");
        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Pinned Note".to_string()),
                body: "Pinned".to_string(),
                is_pinned: true,
                ..Default::default()
            })
            .unwrap();

        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/notes", user.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].body, "Regular");
    }

    #[test]
    fn test_notes_index_search() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        insert_test_note(
            &ctx,
            user.id,
            Some("Meeting Notes"),
            "Meeting notes from today",
        );
        insert_test_note(
            &ctx,
            user.id,
            Some("Shopping List"),
            "Shopping list for tomorrow",
        );

        // Search by body
        let res = router.handle(
            Request::get(format!(
                "http://localhost/api/users/{}/notes?q=meeting",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].body, "Meeting notes from today");

        // Search by title
        let res = router.handle(
            Request::get(format!(
                "http://localhost/api/users/{}/notes?q=Shopping",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("Shopping List".to_string()));
    }

    #[test]
    fn test_notes_index_search_by_title_only() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        insert_test_note(&ctx, user.id, Some("ProjectAlpha"), "Some content");
        insert_test_note(&ctx, user.id, None, "Some other content");

        let res = router.handle(
            Request::get(format!(
                "http://localhost/api/users/{}/notes?q=ProjectAlpha",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("ProjectAlpha".to_string()));
    }

    #[test]
    fn test_notes_index_fts5_search() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        insert_test_note(&ctx, user.id, Some("Alice Smith"), "Notes from Alice");
        insert_test_note(&ctx, user.id, Some("Alice Johnson"), "Notes from Alice");
        insert_test_note(&ctx, user.id, Some("Bob Smith"), "Notes from Bob");
        insert_test_note(&ctx, user.id, Some("Carol White"), "Notes from Carol");

        let base = format!("http://localhost/api/users/{}/notes", user.id);

        // Prefix search
        let res = router.handle(
            Request::get(format!("{base}?q=Al*"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        assert_eq!(
            serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
                .unwrap()
                .data
                .len(),
            2
        );

        // AND search
        let res = router.handle(
            Request::get(format!("{base}?q=Alice AND Smith"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("Alice Smith".to_string()));

        // OR search
        let res = router.handle(
            Request::get(format!("{base}?q=Alice OR Bob"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        assert_eq!(
            serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
                .unwrap()
                .data
                .len(),
            3
        );

        // NOT search
        let res = router.handle(
            Request::get(format!("{base}?q=Alice NOT Smith"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("Alice Johnson".to_string()));

        // Phrase search
        let res = router.handle(
            Request::get(format!("{base}?q=\"Alice Smith\""))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("Alice Smith".to_string()));

        // Column-scoped search (body field only)
        let res = router.handle(
            Request::get(format!("{base}?q=body:Carol"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("Carol White".to_string()));
    }

    #[test]
    fn test_notes_index_pagination() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        for i in 1..=30 {
            insert_test_note(
                &ctx,
                user.id,
                Some(&format!("Note {i}")),
                &format!("Note number {i}"),
            );
        }

        let base = format!("http://localhost/api/users/{}/notes", user.id);

        let res = router.handle(
            Request::get(format!("{base}?limit=10&page=1"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 10);
        assert_eq!(response.pagination.page, 1);
        assert_eq!(response.pagination.limit, 10);
        assert_eq!(response.pagination.total, 30);

        let res = router.handle(
            Request::get(format!("{base}?limit=5&page=2"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 5);
        assert_eq!(response.pagination.page, 2);
        assert_eq!(response.pagination.limit, 5);
        assert_eq!(response.pagination.total, 30);
    }

    #[test]
    fn test_notes_create() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        let res = router.handle(
            Request::post(format!("http://localhost/api/users/{}/notes", user.id))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(r#"{"title":"Test Note","body":"This is a new note","isPinned":false}"#),
        );
        assert_eq!(res.status, Status::Created);
        let note = serde_json::from_slice::<api::Note>(&res.body).unwrap();
        assert_eq!(note.title, Some("Test Note".to_string()));
        assert_eq!(note.body, "This is a new note");
        assert_eq!(note.user_id, user.id);
    }

    #[test]
    fn test_notes_create_forbidden_for_normal_user() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        let res = router.handle(
            Request::post("http://localhost/api/notes")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(format!(r#"{{"userId":"{}","body":"test"}}"#, user.id)),
        );
        assert_eq!(res.status, Status::Forbidden);
    }

    #[test]
    fn test_notes_create_admin_for_target_user() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_admin, admin_token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let (target_user, _) = create_test_user_with_session(&ctx);

        let res = router.handle(Request::post("http://localhost/api/notes")
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("Content-Type", "application/json")
                .body(format!(
                    r#"{{"userId":"{}","title":"Admin Note","body":"Created for target","isPinned":false}}"#,
                    target_user.id
                )),
        );
        assert_eq!(res.status, Status::Created);
        let note = serde_json::from_slice::<api::Note>(&res.body).unwrap();
        assert_eq!(note.user_id, target_user.id);
        assert_eq!(note.body, "Created for target");
    }

    #[test]
    fn test_notes_create_admin_unknown_user() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_admin, admin_token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);

        let res = router.handle(
            Request::post("http://localhost/api/notes")
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("Content-Type", "application/json")
                .body(format!(
                    r#"{{"userId":"{}","body":"note"}}"#,
                    Uuid::now_v7()
                )),
        );
        assert_eq!(res.status, Status::NotFound);
    }

    #[test]
    fn test_notes_show() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);
        let note = insert_test_note(&ctx, user.id, Some("Important"), "My important note");

        let res = router.handle(
            Request::get(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let note = serde_json::from_slice::<api::Note>(&res.body).unwrap();
        assert_eq!(note.body, "My important note");
    }

    #[test]
    fn test_notes_show_not_found() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token) = create_test_user_with_session(&ctx);

        let res = router.handle(
            Request::get(format!("http://localhost/api/notes/{}", Uuid::now_v7()))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::NotFound);
    }

    #[test]
    fn test_notes_update() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);
        let note = insert_test_note(
            &ctx,
            user.id,
            Some("Original Title"),
            "Original note content",
        );

        let res = router.handle(Request::put(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(r#"{"title":"Updated Title","body":"Updated note content","isPinned":false,"isArchived":false,"isTrashed":false}"#),
        );
        assert_eq!(res.status, Status::Ok);
        let note = serde_json::from_slice::<api::Note>(&res.body).unwrap();
        assert_eq!(note.title, Some("Updated Title".to_string()));
        assert_eq!(note.body, "Updated note content");
    }

    #[test]
    fn test_notes_update_validation_error() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);
        let note = insert_test_note(
            &ctx,
            user.id,
            Some("Original Title"),
            "Original note content",
        );

        let res = router.handle(Request::put(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(r#"{"title":"Test","body":"","isPinned":false,"isArchived":false,"isTrashed":false}"#),
        );
        assert_eq!(res.status, Status::BadRequest);
    }

    #[test]
    fn test_notes_delete() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);
        let note = insert_test_note(&ctx, user.id, Some("To Delete"), "Note to be deleted");

        let res = router.handle(
            Request::delete(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::NoContent);

        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/notes", user.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        assert!(
            serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
                .unwrap()
                .data
                .is_empty()
        );
    }

    #[test]
    fn test_notes_index_admin_can_see_all_notes() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_admin, admin_token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let (user1, _) = create_test_user_with_session(&ctx);
        let (user2, _) = create_test_user_with_session(&ctx);

        insert_test_note(&ctx, user1.id, Some("User 1 Note"), "User 1's note");
        insert_test_note(&ctx, user2.id, Some("User 2 Note"), "User 2's note");

        let res = router.handle(
            Request::get("http://localhost/api/notes")
                .header("Authorization", format!("Bearer {admin_token}")),
        );
        assert_eq!(res.status, Status::Ok);
        assert_eq!(
            serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
                .unwrap()
                .data
                .len(),
            2
        );
    }

    #[test]
    fn test_notes_show_admin_can_access_any_note() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_admin, admin_token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let (user, _) = create_test_user_with_session(&ctx);
        let note = insert_test_note(&ctx, user.id, Some("Private Note"), "User's private note");

        let res = router.handle(
            Request::get(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {admin_token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let fetched_note = serde_json::from_slice::<api::Note>(&res.body).unwrap();
        assert_eq!(fetched_note.id, note.id);
        assert_eq!(fetched_note.body, "User's private note");
    }

    #[test]
    fn test_notes_update_admin_can_update_any_note() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_admin, admin_token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let (user, _) = create_test_user_with_session(&ctx);
        let note = insert_test_note(&ctx, user.id, Some("Original Title"), "Original content");

        let res = router.handle(Request::put(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("Content-Type", "application/json")
                .body(r#"{"title":"Admin Title","body":"Admin updated this","isPinned":false,"isArchived":false,"isTrashed":false}"#),
        );
        assert_eq!(res.status, Status::Ok);
        let updated_note = serde_json::from_slice::<api::Note>(&res.body).unwrap();
        assert_eq!(updated_note.body, "Admin updated this");
    }

    #[test]
    fn test_notes_update_admin_can_reassign_user_id() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_admin, admin_token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let (user1, _) = create_test_user_with_session(&ctx);
        let (user2, _) = create_test_user_with_session(&ctx);
        let note = insert_test_note(&ctx, user1.id, Some("Title"), "content");

        let res = router.handle(Request::put(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("Content-Type", "application/json")
                .body(format!(
                    r#"{{"userId":"{}","body":"content","isPinned":false,"isArchived":false,"isTrashed":false}}"#,
                    user2.id
                )),
        );
        assert_eq!(res.status, Status::Ok);
        let updated_note = serde_json::from_slice::<api::Note>(&res.body).unwrap();
        assert_eq!(updated_note.user_id, user2.id);
    }

    #[test]
    fn test_notes_update_user_cannot_reassign_user_id() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user1, token1) = create_test_user_with_session(&ctx);
        let (user2, _) = create_test_user_with_session(&ctx);
        let note = insert_test_note(&ctx, user1.id, Some("Title"), "content");

        let res = router.handle(Request::put(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {token1}"))
                .header("Content-Type", "application/json")
                .body(format!(
                    r#"{{"userId":"{}","body":"content","isPinned":false,"isArchived":false,"isTrashed":false}}"#,
                    user2.id
                )),
        );
        assert_eq!(res.status, Status::Forbidden);
    }

    #[test]
    fn test_notes_delete_admin_can_delete_any_note() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_admin, admin_token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let (user, _) = create_test_user_with_session(&ctx);
        let note = insert_test_note(&ctx, user.id, Some("To Delete"), "Note to delete");

        let res = router.handle(
            Request::delete(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {admin_token}")),
        );
        assert_eq!(res.status, Status::NoContent);

        let res = router.handle(
            Request::get(format!("http://localhost/api/notes/{}", note.id))
                .header("Authorization", format!("Bearer {admin_token}")),
        );
        assert_eq!(res.status, Status::NotFound);
    }

    #[test]
    fn test_notes_index_user_isolation() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user1, token1) = create_test_user_with_session(&ctx);
        let (user2, token2) = create_test_user_with_session(&ctx);

        insert_test_note(&ctx, user1.id, Some("User 1 Note"), "User 1's private note");
        insert_test_note(&ctx, user2.id, Some("User 2 Note"), "User 2's private note");

        // User 1 should only see their own notes via user-scoped route
        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/notes", user1.id))
                .header("Authorization", format!("Bearer {token1}")),
        );
        assert_eq!(res.status, Status::Ok);
        let notes = serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
            .unwrap()
            .data;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].body, "User 1's private note");
        assert_eq!(notes[0].user_id, user1.id);

        // User 2 should only see their own notes via user-scoped route
        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/notes", user2.id))
                .header("Authorization", format!("Bearer {token2}")),
        );
        assert_eq!(res.status, Status::Ok);
        let notes = serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
            .unwrap()
            .data;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].body, "User 2's private note");
        assert_eq!(notes[0].user_id, user2.id);

        // User 1 cannot see User 2's notes via user-scoped route
        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/notes", user2.id))
                .header("Authorization", format!("Bearer {token1}")),
        );
        assert_eq!(res.status, Status::Forbidden);
    }

    #[test]
    fn test_notes_show_user_cannot_access_other_user_note() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user1, token1) = create_test_user_with_session(&ctx);
        let (_, token2) = create_test_user_with_session(&ctx);
        let note1 = insert_test_note(
            &ctx,
            user1.id,
            Some("Private Note"),
            "User 1's private note",
        );

        // User 2 should not be able to access User 1's note
        let res = router.handle(
            Request::get(format!("http://localhost/api/notes/{}", note1.id))
                .header("Authorization", format!("Bearer {token2}")),
        );
        assert_eq!(res.status, Status::NotFound);

        // User 1 should still be able to access their own note
        let res = router.handle(
            Request::get(format!("http://localhost/api/notes/{}", note1.id))
                .header("Authorization", format!("Bearer {token1}")),
        );
        assert_eq!(res.status, Status::Ok);
    }

    #[test]
    fn test_notes_pinned() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Pinned Note".to_string()),
                body: "This is a pinned note".to_string(),
                is_pinned: true,
                ..Default::default()
            })
            .unwrap();
        insert_test_note(
            &ctx,
            user.id,
            Some("Unpinned Note"),
            "This is an unpinned note",
        );

        let res = router.handle(
            Request::get(format!(
                "http://localhost/api/users/{}/notes/pinned",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert!(response.data[0].is_pinned);
        assert_eq!(response.data[0].body, "This is a pinned note");
    }

    #[test]
    fn test_notes_archived() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Archived Note".to_string()),
                body: "This is an archived note".to_string(),
                is_archived: true,
                ..Default::default()
            })
            .unwrap();
        insert_test_note(&ctx, user.id, Some("Active Note"), "This is an active note");

        let res = router.handle(
            Request::get(format!(
                "http://localhost/api/users/{}/notes/archived",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert!(response.data[0].is_archived);
        assert_eq!(response.data[0].body, "This is an archived note");
    }

    #[test]
    fn test_notes_trashed() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Trashed Note".to_string()),
                body: "This is a trashed note".to_string(),
                is_trashed: true,
                ..Default::default()
            })
            .unwrap();
        insert_test_note(&ctx, user.id, Some("Kept Note"), "This is a kept note");

        let res = router.handle(
            Request::get(format!(
                "http://localhost/api/users/{}/notes/trashed",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert!(response.data[0].is_trashed);
        assert_eq!(response.data[0].body, "This is a trashed note");
    }

    #[test]
    fn test_notes_pinned_search() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Pinned Alpha".to_string()),
                body: "Content about alpha".to_string(),
                is_pinned: true,
                ..Default::default()
            })
            .unwrap();
        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Pinned Beta".to_string()),
                body: "Content about beta".to_string(),
                is_pinned: true,
                ..Default::default()
            })
            .unwrap();
        // Non-pinned note that also matches - must not appear
        insert_test_note(&ctx, user.id, Some("Unpinned Alpha"), "Content about alpha");

        let base = format!("http://localhost/api/users/{}/notes/pinned", user.id);

        let res = router.handle(
            Request::get(format!("{base}?q=alpha"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert!(response.data[0].is_pinned);
        assert_eq!(response.data[0].title, Some("Pinned Alpha".to_string()));

        let res = router.handle(
            Request::get(format!("{base}?q=beta"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("Pinned Beta".to_string()));

        let res =
            router.handle(Request::get(base).header("Authorization", format!("Bearer {token}")));
        assert_eq!(res.status, Status::Ok);
        assert_eq!(
            serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
                .unwrap()
                .data
                .len(),
            2
        );
    }

    #[test]
    fn test_notes_archived_search() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Archived Recipe".to_string()),
                body: "How to bake bread".to_string(),
                is_archived: true,
                ..Default::default()
            })
            .unwrap();
        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Archived Travel".to_string()),
                body: "Trip to Paris".to_string(),
                is_archived: true,
                ..Default::default()
            })
            .unwrap();
        // Non-archived note that also matches - must not appear
        insert_test_note(&ctx, user.id, Some("Active Recipe"), "How to bake bread");

        let base = format!("http://localhost/api/users/{}/notes/archived", user.id);

        let res = router.handle(
            Request::get(format!("{base}?q=recipe"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert!(response.data[0].is_archived);
        assert_eq!(response.data[0].title, Some("Archived Recipe".to_string()));

        let res = router.handle(
            Request::get(format!("{base}?q=paris"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("Archived Travel".to_string()));

        let res = router.handle(
            Request::get(format!("{base}?q=zzznomatch"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        assert_eq!(
            serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
                .unwrap()
                .data
                .len(),
            0
        );
    }

    #[test]
    fn test_notes_trashed_search() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Trashed Invoice".to_string()),
                body: "Invoice for January".to_string(),
                is_trashed: true,
                ..Default::default()
            })
            .unwrap();
        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Trashed Draft".to_string()),
                body: "Draft blog post".to_string(),
                is_trashed: true,
                ..Default::default()
            })
            .unwrap();
        // Non-trashed note that also matches - must not appear
        insert_test_note(
            &ctx,
            user.id,
            Some("Active Invoice"),
            "Invoice for February",
        );

        let base = format!("http://localhost/api/users/{}/notes/trashed", user.id);

        let res = router.handle(
            Request::get(format!("{base}?q=invoice"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert!(response.data[0].is_trashed);
        assert_eq!(response.data[0].title, Some("Trashed Invoice".to_string()));

        let res = router.handle(
            Request::get(format!("{base}?q=draft"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("Trashed Draft".to_string()));

        let res = router.handle(
            Request::get(format!("{base}?q=zzznomatch"))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        assert_eq!(
            serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
                .unwrap()
                .data
                .len(),
            0
        );
    }

    #[test]
    fn test_notes_trashed_clear() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Trashed A".to_string()),
                body: "First trashed note".to_string(),
                is_trashed: true,
                ..Default::default()
            })
            .unwrap();
        ctx.database
            .insert_note(Note {
                user_id: user.id,
                title: Some("Trashed B".to_string()),
                body: "Second trashed note".to_string(),
                is_trashed: true,
                ..Default::default()
            })
            .unwrap();
        insert_test_note(&ctx, user.id, Some("Active"), "Non-trashed note");

        let res = router.handle(
            Request::delete(format!(
                "http://localhost/api/users/{}/notes/trashed/clear",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::NoContent);

        let res = router.handle(
            Request::get(format!(
                "http://localhost/api/users/{}/notes/trashed",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        assert!(
            serde_json::from_slice::<api::NoteIndexResponse>(&res.body)
                .unwrap()
                .data
                .is_empty()
        );

        // Non-trashed note must still exist
        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/notes", user.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::NoteIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].title, Some("Active".to_string()));
    }

    #[test]
    fn test_notes_trashed_clear_only_own_notes() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user1, token1) = create_test_user_with_session(&ctx);
        let (user2, _) = create_test_user_with_session(&ctx);

        ctx.database
            .insert_note(Note {
                user_id: user1.id,
                title: Some("User1 Trashed".to_string()),
                body: "User1 trashed note".to_string(),
                is_trashed: true,
                ..Default::default()
            })
            .unwrap();
        ctx.database
            .insert_note(Note {
                user_id: user2.id,
                title: Some("User2 Trashed".to_string()),
                body: "User2 trashed note".to_string(),
                is_trashed: true,
                ..Default::default()
            })
            .unwrap();

        // User-scoped clear only deletes user1's trashed notes
        let res = router.handle(
            Request::delete(format!(
                "http://localhost/api/users/{}/notes/trashed/clear",
                user1.id
            ))
            .header("Authorization", format!("Bearer {token1}")),
        );
        assert_eq!(res.status, Status::NoContent);

        let remaining: i64 = ctx
            .database
            .query("SELECT COUNT(id) FROM notes WHERE is_trashed = 1", ())
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn test_notes_reorder() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        let note1 = Note {
            user_id: user.id,
            title: Some("Note 1".to_string()),
            body: "First note".to_string(),
            position: 0,
            ..Default::default()
        };
        ctx.database.insert_note(note1.clone()).unwrap();
        let note2 = Note {
            user_id: user.id,
            title: Some("Note 2".to_string()),
            body: "Second note".to_string(),
            position: 1,
            ..Default::default()
        };
        ctx.database.insert_note(note2.clone()).unwrap();
        let note3 = Note {
            user_id: user.id,
            title: Some("Note 3".to_string()),
            body: "Third note".to_string(),
            position: 2,
            ..Default::default()
        };
        ctx.database.insert_note(note3.clone()).unwrap();

        // Reorder: 3, 1, 2
        let res = router.handle(
            Request::put(format!(
                "http://localhost/api/users/{}/notes/reorder",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(format!(
                r#"{{"ids":["{}","{}","{}"]}}"#,
                note3.id, note1.id, note2.id
            )),
        );
        assert_eq!(res.status, Status::NoContent);

        let notes = serde_json::from_slice::<api::NoteIndexResponse>(
            &router
                .handle(
                    Request::get(format!("http://localhost/api/users/{}/notes", user.id))
                        .header("Authorization", format!("Bearer {token}")),
                )
                .body,
        )
        .unwrap()
        .data;
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[0].id, note3.id);
        assert_eq!(notes[1].id, note1.id);
        assert_eq!(notes[2].id, note2.id);
    }

    #[test]
    fn test_notes_reorder_partial() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        let note1 = Note {
            user_id: user.id,
            title: Some("Note 1".to_string()),
            body: "First".to_string(),
            position: 0,
            ..Default::default()
        };
        ctx.database.insert_note(note1.clone()).unwrap();
        let note2 = Note {
            user_id: user.id,
            title: Some("Note 2".to_string()),
            body: "Second".to_string(),
            position: 1,
            ..Default::default()
        };
        ctx.database.insert_note(note2.clone()).unwrap();
        let note3 = Note {
            user_id: user.id,
            title: Some("Note 3".to_string()),
            body: "Third".to_string(),
            position: 2,
            ..Default::default()
        };
        ctx.database.insert_note(note3.clone()).unwrap();
        let note4 = Note {
            user_id: user.id,
            title: Some("Note 4".to_string()),
            body: "Fourth".to_string(),
            position: 3,
            ..Default::default()
        };
        ctx.database.insert_note(note4.clone()).unwrap();

        // Reorder only first page: swap note1 and note2
        let res = router.handle(
            Request::put(format!(
                "http://localhost/api/users/{}/notes/reorder",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(format!(r#"{{"ids":["{}","{}"]}}"#, note2.id, note1.id)),
        );
        assert_eq!(res.status, Status::NoContent);

        let notes = serde_json::from_slice::<api::NoteIndexResponse>(
            &router
                .handle(
                    Request::get(format!("http://localhost/api/users/{}/notes", user.id))
                        .header("Authorization", format!("Bearer {token}")),
                )
                .body,
        )
        .unwrap()
        .data;
        assert_eq!(notes.len(), 4);
        assert_eq!(notes[0].id, note2.id);
        assert_eq!(notes[1].id, note1.id);
        assert_eq!(notes[2].id, note3.id);
        assert_eq!(notes[3].id, note4.id);
    }

    #[test]
    fn test_notes_reorder_unauthenticated() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        let res = router.handle(
            Request::put("http://localhost/api/notes/reorder")
                .header("Content-Type", "application/json")
                .body(r#"{"ids":[]}"#),
        );
        assert_eq!(res.status, Status::Unauthorized);
    }

    #[test]
    fn test_notes_reorder_ignores_other_users_notes() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user1, token1) = create_test_user_with_session(&ctx);
        let (user2, _) = create_test_user_with_session(&ctx);

        let note1 = Note {
            user_id: user1.id,
            body: "User 1 note".to_string(),
            position: 0,
            ..Default::default()
        };
        ctx.database.insert_note(note1.clone()).unwrap();
        let note2 = Note {
            user_id: user2.id,
            body: "User 2 note".to_string(),
            position: 0,
            ..Default::default()
        };
        ctx.database.insert_note(note2.clone()).unwrap();

        // User 1 tries to include user 2's note - should be silently ignored
        let res = router.handle(
            Request::put(format!(
                "http://localhost/api/users/{}/notes/reorder",
                user1.id
            ))
            .header("Authorization", format!("Bearer {token1}"))
            .header("Content-Type", "application/json")
            .body(format!(r#"{{"ids":["{}","{}"]}}"#, note2.id, note1.id)),
        );
        assert_eq!(res.status, Status::NoContent);

        let notes = serde_json::from_slice::<api::NoteIndexResponse>(
            &router
                .handle(
                    Request::get(format!("http://localhost/api/users/{}/notes", user1.id))
                        .header("Authorization", format!("Bearer {token1}")),
                )
                .body,
        )
        .unwrap()
        .data;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, note1.id);
    }

    #[test]
    fn test_notes_reorder_pinned_category() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        let normal1 = Note {
            user_id: user.id,
            body: "Normal 1".to_string(),
            position: 0,
            ..Default::default()
        };
        ctx.database.insert_note(normal1.clone()).unwrap();
        let pinned1 = Note {
            user_id: user.id,
            body: "Pinned 1".to_string(),
            is_pinned: true,
            position: 0,
            ..Default::default()
        };
        ctx.database.insert_note(pinned1.clone()).unwrap();
        let pinned2 = Note {
            user_id: user.id,
            body: "Pinned 2".to_string(),
            is_pinned: true,
            position: 1,
            ..Default::default()
        };
        ctx.database.insert_note(pinned2.clone()).unwrap();

        // Swap pinned2 before pinned1
        let res = router.handle(
            Request::put(format!(
                "http://localhost/api/users/{}/notes/pinned/reorder",
                user.id
            ))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(format!(r#"{{"ids":["{}","{}"]}}"#, pinned2.id, pinned1.id)),
        );
        assert_eq!(res.status, Status::NoContent);

        let pinned = serde_json::from_slice::<api::NoteIndexResponse>(
            &router
                .handle(
                    Request::get(format!(
                        "http://localhost/api/users/{}/notes/pinned",
                        user.id
                    ))
                    .header("Authorization", format!("Bearer {token}")),
                )
                .body,
        )
        .unwrap()
        .data;
        assert_eq!(pinned.len(), 2);
        assert_eq!(pinned[0].id, pinned2.id);
        assert_eq!(pinned[1].id, pinned1.id);

        // Normal list should be unaffected
        let normal = serde_json::from_slice::<api::NoteIndexResponse>(
            &router
                .handle(
                    Request::get(format!("http://localhost/api/users/{}/notes", user.id))
                        .header("Authorization", format!("Bearer {token}")),
                )
                .body,
        )
        .unwrap()
        .data;
        assert_eq!(normal.len(), 1);
        assert_eq!(normal[0].id, normal1.id);
    }

    #[test]
    fn test_notes_trash_resets_position() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session(&ctx);

        let note1 = Note {
            user_id: user.id,
            body: "Note 1".to_string(),
            position: 5,
            ..Default::default()
        };
        ctx.database.insert_note(note1.clone()).unwrap();

        let res = router.handle(
            Request::put(format!("http://localhost/api/notes/{}", note1.id))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(r#"{"body":"Note 1","isPinned":false,"isArchived":false,"isTrashed":true}"#),
        );
        assert_eq!(res.status, Status::Ok);
        let updated = serde_json::from_slice::<api::Note>(&res.body).unwrap();
        assert_eq!(updated.position, 0);
    }
}
