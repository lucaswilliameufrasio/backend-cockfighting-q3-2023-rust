use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio_postgres::NoTls;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Domain
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct Person {
    id: Uuid,
    apelido: String,
    nome: String,
    nascimento: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stack: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CreatePerson {
    apelido: String,
    nome: String,
    nascimento: String,
    stack: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct AppState {
    db: Pool,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response<String> {
    let json = serde_json::to_string(body).unwrap();
    let mut resp = Response::new(json);
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        "application/json".parse().unwrap(),
    );
    resp
}

fn empty_response(status: StatusCode) -> Response<String> {
    let mut resp = Response::new(String::new());
    *resp.status_mut() = status;
    resp
}

fn error_response(status: StatusCode, msg: &str) -> Response<String> {
    json_response(status, &serde_json::json!({ "message": msg }))
}

fn is_date_valid(date: &str) -> bool {
    if date.len() != 10 || date.as_bytes()[4] != b'-' || date.as_bytes()[7] != b'-' {
        return false;
    }
    let y: i32 = match date[0..4].parse() { Ok(v) => v, _ => return false };
    let m: u32 = match date[5..7].parse() { Ok(v) => v, _ => return false };
    let d: u32 = match date[8..10].parse() { Ok(v) => v, _ => return false };
    if y < 1800 || y > 9999 || m < 1 || m > 12 || d < 1 || d > 31 {
        return false;
    }
    if m == 2 {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        return if leap { d <= 29 } else { d <= 28 };
    }
    if matches!(m, 4 | 6 | 9 | 11) {
        return d <= 30;
    }
    true
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn handle_request(
    req: Request<Incoming>,
    state: Arc<AppState>,
) -> Result<Response<String>, hyper::Error> {
    let method = req.method();
    let path = req.uri().path();
    let query = req.uri().query().unwrap_or("");

    match (method, path) {
        (&Method::GET, "/health-check") => Ok(empty_response(StatusCode::OK)),

        (&Method::POST, "/pessoas") => handle_create(req, &state.db).await,

        (&Method::GET, path) if path.starts_with("/pessoas/") => {
            let id = &path["/pessoas/".len()..];
            handle_get_by_id(id, &state.db).await
        }

        (&Method::GET, "/pessoas") => {
            let params: HashMap<_, _> = url_params(query);
            match params.get("t") {
                Some(term) => handle_search(term, &state.db).await,
                None => Ok(error_response(StatusCode::BAD_REQUEST, "Missing term")),
            }
        }

        (&Method::GET, "/contagem-pessoas") => handle_count(&state.db).await,

        _ => Ok(error_response(
            StatusCode::NOT_FOUND,
            "Route not found",
        )),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_create(
    req: Request<Incoming>,
    db: &Pool,
) -> Result<Response<String>, hyper::Error> {
    let body_bytes = req.collect().await.map(|c| c.to_bytes()).unwrap_or_default();

    let input: CreatePerson = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(_) => return Ok(error_response(StatusCode::BAD_REQUEST, "Invalid JSON")),
    };

    if input.apelido.is_empty()
        || input.nome.is_empty()
        || !is_date_valid(&input.nascimento)
    {
        return Ok(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Invalid fields",
        ));
    }
    if input.apelido.len() > 32 || input.nome.len() > 100 {
        return Ok(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Field too long",
        ));
    }
    if let Some(ref stack) = input.stack {
        for s in stack {
            if s.len() > 32 {
                return Ok(error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Stack item too long",
                ));
            }
        }
    }

    let id = Uuid::now_v7();

    let stack_val: Vec<String> = input.stack.clone().unwrap_or_default();

    let result = db
        .get()
        .await
        .unwrap()
        .execute(
            "INSERT INTO people (id, nickname, name, birth_date, stack) VALUES ($1, $2, $3, TO_DATE($4, 'YYYY-MM-DD'), $5) ON CONFLICT (nickname) DO NOTHING",
            &[&id, &input.apelido, &input.nome, &input.nascimento, &stack_val],
        )
        .await;

    match result {
        Ok(rows) if rows > 0 => {
            let mut resp = json_response(
                StatusCode::CREATED,
                &serde_json::json!({ "id": id }),
            );
            resp.headers_mut().insert(
                "Location",
                format!("/pessoas/{}", id).parse().unwrap(),
            );
            Ok(resp)
        }
        Ok(_) => Ok(error_response(StatusCode::UNPROCESSABLE_ENTITY, "Conflict")),
        Err(e) => {
            eprintln!("DB error: {:?}", e);
            Ok(error_response(StatusCode::UNPROCESSABLE_ENTITY, &format!("DB error: {}", e)))
        }
    }
}

async fn handle_get_by_id(id: &str, db: &Pool) -> Result<Response<String>, hyper::Error> {
    if id.len() != 36 {
        return Ok(error_response(StatusCode::NOT_FOUND, "Not found"));
    }
    let uuid: Uuid = match id.parse() {
        Ok(v) => v,
        Err(_) => return Ok(error_response(StatusCode::NOT_FOUND, "Not found")),
    };

    let row = db
        .get()
        .await
        .unwrap()
        .query_opt("SELECT id, nickname, name, birth_date::text, stack FROM people WHERE id = $1", &[&uuid])
        .await
        .unwrap();

    match row {
        Some(r) => {
            let p = Person {
                id: r.get(0),
                apelido: r.get(1),
                nome: r.get(2),
                nascimento: r.get(3),
                stack: r.get(4),
            };
            Ok(json_response(StatusCode::OK, &p))
        }
        None => Ok(error_response(StatusCode::NOT_FOUND, "Not found")),
    }
}

async fn handle_search(
    term: &str,
    db: &Pool,
) -> Result<Response<String>, hyper::Error> {
    let pattern = format!("%{}%", term);

    let rows = db
        .get()
        .await
        .unwrap()
        .query(
            "SELECT id, nickname, name, birth_date::text, stack FROM people WHERE searchable LIKE $1 LIMIT 50",
            &[&pattern],
        )
        .await
        .unwrap();

    let people: Vec<Person> = rows
        .iter()
        .map(|r| Person {
            id: r.get(0),
            apelido: r.get(1),
            nome: r.get(2),
            nascimento: r.get(3),
            stack: r.get(4),
        })
        .collect();

    Ok(json_response(StatusCode::OK, &people))
}

async fn handle_count(db: &Pool) -> Result<Response<String>, hyper::Error> {
    let row = db
        .get()
        .await
        .unwrap()
        .query_one("SELECT COUNT(*) AS count FROM people", &[])
        .await
        .unwrap();

    let count: i64 = row.get(0);
    Ok(json_response(
        StatusCode::OK,
        &serde_json::json!({ "count": count }),
    ))
}

// ---------------------------------------------------------------------------
// Util
// ---------------------------------------------------------------------------

fn url_params(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.to_string(), url_decode(v));
        }
    }
    map
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().and_then(|c| (c as char).to_digit(16)).unwrap_or(0);
            let lo = chars.next().and_then(|c| (c as char).to_digit(16)).unwrap_or(0);
            out.push((hi as u8 * 16 + lo as u8) as char);
        } else if b == b'+' {
            out.push(' ');
        } else {
            out.push(b as char);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);

    let db_host = std::env::var("DB_HOST").unwrap_or_else(|_| "localhost".into());
    let db_port: u16 = std::env::var("DB_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5432);
    let db_name = std::env::var("DB_NAME").unwrap_or_else(|_| "fight".into());
    let db_user = std::env::var("DB_USER").unwrap_or_else(|_| "postgres".into());
    let db_password = std::env::var("DB_PASSWORD").unwrap_or_else(|_| "fight".into());

    let mut cfg = Config::new();
    cfg.host = Some(db_host);
    cfg.port = Some(db_port);
    cfg.dbname = Some(db_name);
    cfg.user = Some(db_user);
    cfg.password = Some(db_password);
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });

    let pool = cfg
        .create_pool(Some(Runtime::Tokio1), NoTls)
        .expect("failed to create pool");

    let state = Arc::new(AppState { db: pool });

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let listener = TcpListener::bind(addr).await.unwrap();

    eprintln!("Listening on http://{}", addr);

    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let io = TokioIo::new(stream);
        let state = state.clone();

        tokio::spawn(async move {
            let svc = service_fn(move |req| handle_request(req, state.clone()));
            if let Err(err) = http1::Builder::new()
                .serve_connection(io, svc)
                .await
            {
                eprintln!("Connection error: {}", err);
            }
        });
    }
}
