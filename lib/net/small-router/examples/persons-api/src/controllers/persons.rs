/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use anyhow::Result;
use bsql::{DatabaseType, execute_args, query_args};
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
        let predicate = match ctx.database.database_type() {
            DatabaseType::Sqlite => {
                "id IN (SELECT id FROM persons_fts WHERE persons_fts MATCH :fts_query)"
            }
            DatabaseType::Mysql => "MATCH(name) AGAINST(:fts_query IN BOOLEAN MODE)",
        };
        let fts_query = ctx.database.preprocess_fts_query(q);
        let total = ctx.database.query_some::<i64>(
            format!("SELECT COUNT(id) FROM persons WHERE {predicate}"),
            fts_query.clone(),
        )?;
        let persons = query_args!(
            Person,
            ctx.database,
            format!(
                "SELECT {} FROM persons WHERE {predicate} ORDER BY created_at DESC, id DESC LIMIT :limit OFFSET :offset",
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
    #[ignore = "requires DATABASE_DRIVER=mysql and a dedicated empty MySQL database"]
    fn test_persons_mysql() -> Result<()> {
        let database = crate::open_database(bsql::PoolOptions { max_connections: 2 })?;
        assert_eq!(
            database.database_type(),
            DatabaseType::Mysql,
            "set DATABASE_DRIVER=mysql"
        );
        let datetime = chrono::DateTime::<Utc>::from_timestamp(1_700_000_000, 123_456_789).unwrap();
        let date = datetime.naive_utc().date();
        let (read_date, read_datetime, read_naive) = database.query_some::<(
            chrono::NaiveDate,
            chrono::DateTime<Utc>,
            chrono::NaiveDateTime,
        )>(
            "SELECT CAST(? AS DATE), CAST(? AS DATETIME(6)), CAST(? AS DATETIME(6))",
            (date, datetime, datetime.naive_utc()),
        )?;
        assert_eq!(read_date, date);
        assert_eq!(
            read_datetime.timestamp_micros(),
            datetime.timestamp_micros()
        );
        assert_eq!(read_naive, read_datetime.naive_utc());
        database.transaction(|transaction| -> Result<()> {
            assert_eq!(transaction.database_type(), DatabaseType::Mysql);
            Ok(())
        })?;
        let ctx = Context::with_database(database.clone())?;
        // Reopening must skip migrations that have already been applied.
        Context::with_database(database.clone())?;
        assert_eq!(
            database.query_some::<i64>("SELECT COUNT(*) FROM schema_migrations", ())?,
            1
        );
        let probe = Person {
            name: "Migration Probe".to_string(),
            ..Default::default()
        };
        let result = database.transaction(|transaction| -> Result<()> {
            transaction.insert_person(probe.clone())?;
            transaction.migration(&[])?;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(
            database.query_some::<i64>("SELECT COUNT(*) FROM persons WHERE id = ?", probe.id)?,
            0
        );
        assert_search_syntax(ctx.clone());
        let router = router(ctx);

        let res = router.handle(
            &Request::post("http://localhost/persons")
                .json(json!({ "name": "Alice Smith", "ageInYears": 30, "relation": "me" })),
        );
        assert_eq!(res.status, Status::Created);
        let person = serde_json::from_slice::<api::Person>(&res.body)?;
        let url = format!("http://localhost/persons/{}", person.id);
        let res = router.handle(&Request::get(&url));
        assert_eq!(res.status, Status::Ok);
        let fetched = serde_json::from_slice::<api::Person>(&res.body)?;
        assert_eq!(fetched.id, person.id);
        assert_eq!(
            fetched.created_at.timestamp_micros(),
            person.created_at.timestamp_micros()
        );

        for query in [
            "Alice",
            "Al",
            "Al*",
            "Alice%20Smith",
            "Alice%20AND%20Smith",
            "Alice%20NOT%20Smith",
        ] {
            let res = router.handle(&Request::get(format!(
                "http://localhost/persons?q={query}&limit=1"
            )));
            assert_eq!(res.status, Status::Ok);
            let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body)?;
            let expected = if query == "Alice%20NOT%20Smith" { 0 } else { 1 };
            assert_eq!(response.pagination.total, expected);
            assert_eq!(response.data.len() as i64, expected);
        }

        let res =
            router
                .handle(&Request::put(&url).json(
                    json!({ "name": "Carol White", "ageInYears": 31, "relation": "sister" }),
                ));
        assert_eq!(res.status, Status::Ok);
        let updated = serde_json::from_slice::<api::Person>(&res.body)?;
        assert_eq!(updated.name, "Carol White");
        assert_eq!(updated.age_in_years, 31);
        let res = router.handle(&Request::get("http://localhost/persons?q=Alice"));
        assert_eq!(
            serde_json::from_slice::<api::PersonIndexResponse>(&res.body)?
                .pagination
                .total,
            0
        );
        let res = router.handle(&Request::get("http://localhost/persons?q=Carol"));
        assert_eq!(
            serde_json::from_slice::<api::PersonIndexResponse>(&res.body)?
                .pagination
                .total,
            1
        );
        assert_eq!(
            router.handle(&Request::delete(&url)).status,
            Status::NoContent
        );
        assert_eq!(router.handle(&Request::get(&url)).status, Status::NotFound);
        let res = router.handle(&Request::get("http://localhost/persons?q=Carol"));
        assert_eq!(
            serde_json::from_slice::<api::PersonIndexResponse>(&res.body)?
                .pagination
                .total,
            0
        );
        Ok(())
    }

    #[test]
    fn test_persons_index() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        // Fetch /persons check if empty
        let res = router.handle(&Request::get("http://localhost/persons"));
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
        let res = router.handle(&Request::get("http://localhost/persons"));
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
        let res = router.handle(&Request::get("http://localhost/persons?q=Alice"));
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].name, "Alice");
    }

    #[test]
    fn test_persons_index_search_syntax() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        assert_search_syntax(ctx);
    }

    fn assert_search_syntax(ctx: Context) {
        let names = ["Alice Smith", "Alice Johnson", "Bob Smith", "Carol White"];
        for name in names {
            ctx.database
                .insert_person(Person {
                    name: name.to_string(),
                    ..Default::default()
                })
                .unwrap();
        }
        let router = router(ctx.clone());
        let cases: &[(&str, &[&str])] = &[
            ("Al", &["Alice Johnson", "Alice Smith"]),
            ("Al*", &["Alice Johnson", "Alice Smith"]),
            ("alice", &["Alice Johnson", "Alice Smith"]),
            ("Alice Smith", &["Alice Smith"]),
            ("Alice AND Smith", &["Alice Smith"]),
            (
                "Alice OR Bob",
                &["Alice Johnson", "Alice Smith", "Bob Smith"],
            ),
            ("Alice NOT Smith", &["Alice Johnson"]),
            (
                "Alice OR Bob AND Smith",
                &["Alice Johnson", "Alice Smith", "Bob Smith"],
            ),
            ("Alice AND Smith OR Carol", &["Alice Smith", "Carol White"]),
            ("Alice OR Bob NOT Smith", &["Alice Johnson", "Alice Smith"]),
            ("Alice NOT Bob Smith", &["Alice Johnson", "Alice Smith"]),
            ("Alice NOT Smith NOT Johnson", &[]),
            (
                "Alice NOT Smith OR Bob NOT White",
                &["Alice Johnson", "Bob Smith"],
            ),
            ("Alice Smith NOT Bob", &["Alice Smith"]),
            ("Alice AND Smith NOT Johnson", &["Alice Smith"]),
            ("AND Alice AND OR Smith NOT", &["Alice Smith"]),
            ("(Alice) \"Smith\"", &["Alice Smith"]),
            ("Alice-Smith", &["Alice Smith"]),
            ("OR NOT () * + - @", &[]),
            ("   ", &[]),
            ("Missing", &[]),
            ("AND*", &[]),
        ];
        for (query, expected) in cases {
            let params = serde_urlencoded::to_string([("q", *query)]).unwrap();
            let res = router.handle(&Request::get(format!("http://localhost/persons?{params}")));
            assert_eq!(res.status, Status::Ok, "{query}");
            let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
            let mut actual: Vec<_> = response
                .data
                .iter()
                .map(|person| person.name.as_str())
                .collect();
            actual.sort_unstable();
            assert_eq!(actual, *expected, "{query}");
            assert_eq!(
                response.pagination.total as usize,
                expected.len(),
                "{query}"
            );
            let res = router.handle(&Request::get(format!(
                "http://localhost/persons?{params}&limit=1&page=2"
            )));
            assert_eq!(res.status, Status::Ok, "{query}");
            let response = serde_json::from_slice::<api::PersonIndexResponse>(&res.body).unwrap();
            assert_eq!(
                response.pagination.total as usize,
                expected.len(),
                "{query}"
            );
            assert_eq!(
                response.data.len(),
                usize::from(expected.len() > 1),
                "{query}"
            );
        }
        for separator in [" ", " AND ", " OR "] {
            let query = vec!["Alice"; 200].join(separator);
            let predicate = match ctx.database.database_type() {
                DatabaseType::Sqlite => {
                    "id IN (SELECT id FROM persons_fts WHERE persons_fts MATCH ?)"
                }
                DatabaseType::Mysql => "MATCH(name) AGAINST(? IN BOOLEAN MODE)",
            };
            let total = ctx
                .database
                .query_some::<i64>(
                    format!("SELECT COUNT(*) FROM persons WHERE {predicate}"),
                    ctx.database.preprocess_fts_query(&query),
                )
                .unwrap();
            assert_eq!(total, 2, "{separator}");
            let params = serde_urlencoded::to_string([("q", query)]).unwrap();
            let res = router.handle(&Request::get(format!("http://localhost/persons?{params}")));
            assert_eq!(res.status, Status::BadRequest, "{separator}");
        }
        for character in ["a", "\u{e9}"] {
            for length in [128, 129] {
                let params =
                    serde_urlencoded::to_string([("q", character.repeat(length))]).unwrap();
                let res =
                    router.handle(&Request::get(format!("http://localhost/persons?{params}")));
                assert_eq!(
                    res.status,
                    if length == 128 {
                        Status::Ok
                    } else {
                        Status::BadRequest
                    }
                );
            }
        }
        for name in names {
            ctx.database
                .execute("DELETE FROM persons WHERE name = ?", name.to_string())
                .unwrap();
        }
    }

    #[test]
    fn test_persons_index_invalid_query_params() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        let res = router.handle(&Request::get("http://localhost/persons?page=0"));
        assert_eq!(res.status, Status::BadRequest);

        let res = router.handle(&Request::get("http://localhost/persons?limit=0"));
        assert_eq!(res.status, Status::BadRequest);

        let res = router.handle(&Request::get("http://localhost/persons?limit=51"));
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
        let res = router.handle(&Request::get("http://localhost/persons?limit=5&page=1"));
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
        let res = router.handle(&Request::get("http://localhost/persons?limit=5&page=2"));
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
            &Request::post("http://localhost/persons")
                .json(json!({ "name": "Jan", "ageInYears": 40, "relation": "me" })),
        );
        assert_eq!(res.status, Status::Created);
        let person = serde_json::from_slice::<api::Person>(&res.body).unwrap();
        assert_eq!(person.name, "Jan");

        // Create with invalid JSON
        let res = router.handle(
            &Request::post("http://localhost/persons")
                .header("Content-Type", "application/json")
                .body("not-json"),
        );
        assert_eq!(res.status, Status::BadRequest);

        // Create with age too low
        let res = router.handle(
            &Request::post("http://localhost/persons")
                .json(json!({ "name": "Jan", "ageInYears": 5, "relation": "me" })),
        );
        assert_eq!(res.status, Status::BadRequest);

        // Create with name too short
        let res = router.handle(
            &Request::post("http://localhost/persons")
                .json(json!({ "name": "Jo", "ageInYears": 40, "relation": "me" })),
        );
        assert_eq!(res.status, Status::BadRequest);

        // Create with forbidden name
        let res = router.handle(
            &Request::post("http://localhost/persons")
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
        let res = router.handle(&Request::get(format!(
            "http://localhost/persons/{}",
            person.id
        )));
        assert_eq!(res.status, Status::Ok);
        let person = serde_json::from_slice::<api::Person>(&res.body).unwrap();
        assert_eq!(person.name, "Jan");

        // Fetch other person by random id should be 404 Not Found
        let res = router.handle(&Request::get(format!(
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
            &Request::put(format!("http://localhost/persons/{}", person.id)).urlencoded(&[
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
            &Request::put(format!("http://localhost/persons/{}", person.id)).urlencoded(&[
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
        let res = router.handle(&Request::delete(format!(
            "http://localhost/persons/{}",
            person.id
        )));
        assert_eq!(res.status, Status::NoContent);

        // Fetch /persons check if empty
        let res = router.handle(&Request::get("http://localhost/persons"));
        assert_eq!(res.status, Status::Ok);
        let persons = serde_json::from_slice::<api::PersonIndexResponse>(&res.body)
            .unwrap()
            .data;
        assert!(persons.is_empty());
    }
}
