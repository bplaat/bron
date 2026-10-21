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

use crate::context::{Context, DatabaseHelpers};
use crate::controllers::not_found;
use crate::models::{IndexQuery, Person, Relation};
use crate::{api, validators};

// MARK: Persons Index
pub(crate) fn persons_index(req: &Request, ctx: &Context) -> Result<Response> {
    // Parse request query
    let query = match req.url.query() {
        Some(query) => match serde_urlencoded::from_str::<IndexQuery>(query) {
            Ok(query) => query,
            Err(_) => return Ok(Response::with_status(Status::BadRequest)),
        },
        None => IndexQuery::default(),
    };
    if let Err(report) = query.validate() {
        return Ok(Response::with_status(Status::BadRequest).json(api::Report::from(report)));
    }

    // Get persons
    let (total, persons) = if let Some(q) = query.query.as_deref().filter(|s| !s.is_empty()) {
        let fts_query = preprocess_fts_query(q);
        let total = ctx.database.query_some::<i64>(
            "SELECT COUNT(id) FROM persons WHERE id IN (SELECT id FROM persons_fts WHERE persons_fts MATCH ?)",
            fts_query.clone(),
        )?;
        let persons = query_args!(
            Person,
            ctx.database,
            format!(
                "SELECT {} FROM persons WHERE id IN (SELECT id FROM persons_fts WHERE persons_fts MATCH :fts_query) ORDER BY created_at DESC, id DESC LIMIT :limit OFFSET :offset",
                Person::columns()
            ),
            Args {
                fts_query: fts_query,
                limit: query.limit,
                offset: (query.page - 1) * query.limit
            }
        )?
        .map(|r| r.map(Into::into).map_err(anyhow::Error::from))
        .collect::<Result<Vec<api::Person>>>()?;
        (total, persons)
    } else {
        let total = ctx
            .database
            .query_some::<i64>("SELECT COUNT(id) FROM persons", ())?;
        let persons = query_args!(
            Person,
            ctx.database,
            format!(
                "SELECT {} FROM persons ORDER BY created_at DESC, id DESC LIMIT :limit OFFSET :offset",
                Person::columns()
            ),
            Args {
                limit: query.limit,
                offset: (query.page - 1) * query.limit
            }
        )?
        .map(|r| r.map(Into::into).map_err(anyhow::Error::from))
        .collect::<Result<Vec<api::Person>>>()?;
        (total, persons)
    };

    // Return persons
    Ok(Response::with_json(api::PersonIndexResponse {
        pagination: api::Pagination {
            page: query.page,
            limit: query.limit,
            total,
        },
        data: persons,
    }))
}

// MARK: Persons Create
#[derive(Validate, FromStruct)]
#[from_struct(api::PersonCreateUpdateBody, only_from)]
struct PersonCreateUpdateBody {
    #[validate(ascii, length(min = 3, max = 25), custom(validators::name_validator))]
    name: String,
    #[validate(range(min = 8))]
    age_in_years: i64,
    relation: Relation,
}

pub(crate) fn persons_create(req: &Request, ctx: &Context) -> Result<Response> {
    // Parse and validate body
    let body = match req.parse_body::<api::PersonCreateUpdateBody>() {
        Ok(body) => PersonCreateUpdateBody::from(body),
        Err(status) => return Ok(Response::with_status(status)),
    };
    if let Err(report) = body.validate() {
        return Ok(Response::with_status(Status::BadRequest).json(api::Report::from(report)));
    }

    // Create person
    let person = Person {
        name: body.name,
        age_in_years: body.age_in_years,
        relation: body.relation,
        ..Default::default()
    };
    ctx.database.insert_person(person.clone())?;

    // Return created person
    Ok(Response::with_status(Status::Created).json(api::Person::from(person)))
}

// MARK: Persons Show
pub(crate) fn persons_show(req: &Request, ctx: &Context) -> Result<Response> {
    // Get person
    let person_id = match parse_person_id(req) {
        Ok(id) => id,
        Err(_) => return Ok(Response::with_status(Status::BadRequest)),
    };
    let person = match get_person(person_id, ctx)? {
        Some(person) => person,
        None => return not_found(req, ctx),
    };

    // Return person
    Ok(Response::with_json(api::Person::from(person)))
}

// MARK: Persons Update
pub(crate) fn persons_update(req: &Request, ctx: &Context) -> Result<Response> {
    // Get person
    let person_id = match parse_person_id(req) {
        Ok(id) => id,
        Err(_) => return Ok(Response::with_status(Status::BadRequest)),
    };
    let mut person = match get_person(person_id, ctx)? {
        Some(person) => person,
        None => return not_found(req, ctx),
    };

    // Parse and validate body
    let body = match req.parse_body::<api::PersonCreateUpdateBody>() {
        Ok(body) => PersonCreateUpdateBody::from(body),
        Err(status) => return Ok(Response::with_status(status)),
    };
    if let Err(report) = body.validate() {
        return Ok(Response::with_status(Status::BadRequest).json(api::Report::from(report)));
    }

    // Update person
    person.name = body.name;
    person.age_in_years = body.age_in_years;
    person.relation = body.relation;
    person.updated_at = Utc::now();
    execute_args!(
        ctx.database,
        "UPDATE persons SET name = :name, age = :age, relation = :relation, updated_at = :updated_at WHERE id = :id",
        Args {
            id: person.id,
            name: person.name.clone(),
            age: person.age_in_years,
            relation: person.relation,
            updated_at: person.updated_at
        }
    )?;

    // Return updated person
    Ok(Response::with_json(api::Person::from(person)))
}

// MARK: Persons Delete
pub(crate) fn persons_delete(req: &Request, ctx: &Context) -> Result<Response> {
    // Get person
    let person_id = match parse_person_id(req) {
        Ok(id) => id,
        Err(_) => return Ok(Response::with_status(Status::BadRequest)),
    };
    let person = match get_person(person_id, ctx)? {
        Some(person) => person,
        None => return not_found(req, ctx),
    };

    // Delete person
    ctx.database
        .execute("DELETE FROM persons WHERE id = ?", person.id)?;

    // Success response
    Ok(Response::with_status(Status::NoContent))
}

// MARK: Helpers
fn parse_person_id(req: &Request) -> Result<Uuid> {
    match req
        .params
        .get("person_id")
        .and_then(|id| id.parse::<Uuid>().ok())
    {
        Some(id) => Ok(id),
        None => anyhow::bail!("Invalid UUID"),
    }
}

fn get_person(person_id: Uuid, ctx: &Context) -> Result<Option<Person>> {
    Ok(ctx
        .database
        .query::<Person>(
            format!(
                "SELECT {} FROM persons WHERE id = ? LIMIT 1",
                Person::columns()
            ),
            person_id,
        )?
        .next()
        .transpose()
        .map_err(Box::new)?)
}

// MARK: Tests
#[cfg(test)]
mod test {
    use serde_json::json;

    use super::*;
    use crate::router;

    #[test]
    fn test_persons_index() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        // Fetch /persons check if empty
        let res = router.handle(Request::get("http://localhost/persons"));
        assert_eq!(res.status, Status::Ok);
        let persons = serde_json::from_slice::<api::PersonIndexResponse>(&res.body)
            .unwrap()
            .data;
        assert!(persons.is_empty());

        // Create person
        let person = Person {
            name: "Jan".to_string(),
            age_in_years: 40,
            relation: Relation::Me,
            ..Default::default()
        };
        ctx.database.insert_person(person.clone()).unwrap();

        // Fetch /persons check if person is there
        let res = router.handle(Request::get("http://localhost/persons"));
        assert_eq!(res.status, Status::Ok);
        let persons = serde_json::from_slice::<api::PersonIndexResponse>(&res.body)
            .unwrap()
            .data;
        assert_eq!(persons.len(), 1);
        assert_eq!(persons[0].name, "Jan");
    }

    #[test]
    fn test_persons_index_search() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        // Create multiple persons
        ctx.database
            .insert_person(Person {
                name: "Alice".to_string(),
                ..Default::default()
            })
            .unwrap();
        ctx.database
            .insert_person(Person {
                name: "Bob".to_string(),
                ..Default::default()
            })
            .unwrap();

        // Search for "Alice" (preprocessed to "Alice*")
        let res = router.handle(Request::get("http://localhost/persons?q=Alice"));
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].name, "Alice");
    }

    #[test]
    fn test_persons_index_fts5_search() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        ctx.database
            .insert_person(Person {
                name: "Alice Smith".to_string(),
                ..Default::default()
            })
            .unwrap();
        ctx.database
            .insert_person(Person {
                name: "Alice Johnson".to_string(),
                ..Default::default()
            })
            .unwrap();
        ctx.database
            .insert_person(Person {
                name: "Bob Smith".to_string(),
                ..Default::default()
            })
            .unwrap();
        ctx.database
            .insert_person(Person {
                name: "Carol White".to_string(),
                ..Default::default()
            })
            .unwrap();

        // Prefix search (Al* preserved as-is)
        let res = router.handle(Request::get("http://localhost/persons?q=Al*"));
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 2);

        // AND search
        let res = router.handle(Request::get("http://localhost/persons?q=Alice AND Smith"));
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].name, "Alice Smith");

        // OR search
        let res = router.handle(Request::get("http://localhost/persons?q=Alice OR Bob"));
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 3);

        // NOT search
        let res = router.handle(Request::get("http://localhost/persons?q=Alice NOT Smith"));
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].name, "Alice Johnson");
    }

    #[test]
    fn test_persons_index_invalid_query_params() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        let res = router.handle(Request::get("http://localhost/persons?page=0"));
        assert_eq!(res.status, Status::BadRequest);

        let res = router.handle(Request::get("http://localhost/persons?limit=0"));
        assert_eq!(res.status, Status::BadRequest);

        let res = router.handle(Request::get("http://localhost/persons?limit=51"));
        assert_eq!(res.status, Status::BadRequest);
    }

    #[test]
    fn test_persons_index_pagination() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let created_at = chrono::DateTime::from_timestamp_secs(1_700_000_000).unwrap();

        // Create multiple persons with the same timestamp to test deterministic ordering
        for i in 1_u8..=30 {
            ctx.database
                .insert_person(Person {
                    id: Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, i]),
                    name: format!("Person {i}"),
                    age_in_years: 20 + i64::from(i),
                    relation: Relation::Me,
                    created_at,
                    ..Default::default()
                })
                .unwrap();
        }

        // Fetch /persons with limit 5 and page 1
        let res = router.handle(Request::get("http://localhost/persons?limit=5&page=1"));
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 5);
        assert_eq!(response.pagination.page, 1);
        assert_eq!(response.pagination.limit, 5);
        assert_eq!(response.pagination.total, 30);
        assert_eq!(
            response
                .data
                .iter()
                .map(|person| person.name.as_str())
                .collect::<Vec<_>>(),
            [
                "Person 30",
                "Person 29",
                "Person 28",
                "Person 27",
                "Person 26"
            ]
        );

        // Fetch /persons with limit 5 and page 2
        let res = router.handle(Request::get("http://localhost/persons?limit=5&page=2"));
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 5);
        assert_eq!(response.pagination.page, 2);
        assert_eq!(response.pagination.limit, 5);
        assert_eq!(response.pagination.total, 30);
        assert_eq!(
            response
                .data
                .iter()
                .map(|person| person.name.as_str())
                .collect::<Vec<_>>(),
            [
                "Person 25",
                "Person 24",
                "Person 23",
                "Person 22",
                "Person 21"
            ]
        );
    }

    #[test]
    fn test_persons_create() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        // Create person
        let res = router.handle(
            Request::post("http://localhost/persons")
                .json(json!({ "name": "Jan", "ageInYears": 40, "relation": "me" })),
        );
        assert_eq!(res.status, Status::Created);
        let person = serde_json::from_slice::<api::Person>(&res.body).unwrap();
        assert_eq!(person.name, "Jan");

        // Create with invalid JSON
        let res = router.handle(
            Request::post("http://localhost/persons")
                .header("Content-Type", "application/json")
                .body("not-json"),
        );
        assert_eq!(res.status, Status::BadRequest);

        // Create with age too low
        let res = router.handle(
            Request::post("http://localhost/persons")
                .json(json!({ "name": "Jan", "ageInYears": 5, "relation": "me" })),
        );
        assert_eq!(res.status, Status::BadRequest);

        // Create with name too short
        let res = router.handle(
            Request::post("http://localhost/persons")
                .json(json!({ "name": "Jo", "ageInYears": 40, "relation": "me" })),
        );
        assert_eq!(res.status, Status::BadRequest);

        // Create with forbidden name
        let res = router.handle(
            Request::post("http://localhost/persons")
                .json(json!({ "name": "Bastiaan", "ageInYears": 40, "relation": "me" })),
        );
        assert_eq!(res.status, Status::BadRequest);
    }

    #[test]
    fn test_persons_show() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        // Create person
        let person = Person {
            name: "Jan".to_string(),
            age_in_years: 40,
            relation: Relation::Me,
            ..Default::default()
        };
        ctx.database.insert_person(person.clone()).unwrap();

        // Fetch /persons/:personId check if person is there
        let res = router.handle(Request::get(format!(
            "http://localhost/persons/{}",
            person.id
        )));
        assert_eq!(res.status, Status::Ok);
        let person = serde_json::from_slice::<api::Person>(&res.body).unwrap();
        assert_eq!(person.name, "Jan");

        // Fetch other person by random id should be 404 Not Found
        let res = router.handle(Request::get(format!(
            "http://localhost/persons/{}",
            Uuid::now_v7()
        )));
        assert_eq!(res.status, Status::NotFound);
    }

    #[test]
    fn test_persons_update() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        // Create person
        let person = Person {
            name: "Jan".to_string(),
            age_in_years: 40,
            relation: Relation::Me,
            ..Default::default()
        };
        ctx.database.insert_person(person.clone()).unwrap();

        // Update person
        let res = router.handle(
            Request::put(format!("http://localhost/persons/{}", person.id)).urlencoded(&[
                ("name", "Jan"),
                ("ageInYears", "41"),
                ("relation", "me"),
            ]),
        );
        assert_eq!(res.status, Status::Ok);
        let person = serde_json::from_slice::<api::Person>(&res.body).unwrap();
        assert_eq!(person.age_in_years, 41);

        // Update person with validation errors
        let res = router.handle(
            Request::put(format!("http://localhost/persons/{}", person.id)).urlencoded(&[
                ("name", "Bastiaan"),
                ("ageInYears", "41"),
                ("relation", "wrong"),
            ]),
        );
        assert_eq!(res.status, Status::BadRequest);
    }

    #[test]
    fn test_persons_delete() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        // Create person
        let person = Person {
            name: "Jan".to_string(),
            age_in_years: 40,
            relation: Relation::Me,
            ..Default::default()
        };
        ctx.database.insert_person(person.clone()).unwrap();

        // Delete person
        let res = router.handle(Request::delete(format!(
            "http://localhost/persons/{}",
            person.id
        )));
        assert_eq!(res.status, Status::NoContent);

        // Fetch /persons check if empty
        let res = router.handle(Request::get("http://localhost/persons"));
        assert_eq!(res.status, Status::Ok);
        let persons = serde_json::from_slice::<api::PersonIndexResponse>(&res.body)
            .unwrap()
            .data;
        assert!(persons.is_empty());
    }
}
