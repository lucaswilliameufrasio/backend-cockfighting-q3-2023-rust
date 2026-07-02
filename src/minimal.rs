// Minimal hyper 1.10.1 example — sem banco, só roteamento e JSON
//
// Uso:
//   cargo run --example minimal
//   curl http://localhost:3000/hello
//   curl http://localhost:3000/echo -d '{"msg":"oi"}'

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// JSON helper
// ---------------------------------------------------------------------------

fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response<String> {
    let json = serde_json::to_string(body).unwrap();
    let mut resp = Response::new(json);
    *resp.status_mut() = status;
    resp.headers_mut()
        .insert("content-type", "application/json".parse().unwrap());
    resp
}

fn error(status: StatusCode, msg: &str) -> Response<String> {
    json_response(status, &serde_json::json!({ "error": msg }))
}

// ---------------------------------------------------------------------------
// Counter (state compartilhada)
// ---------------------------------------------------------------------------

struct AppState {
    counter: AtomicU64,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

async fn handle(req: Request<Incoming>, state: &AppState) -> Response<String> {
    let method = req.method();
    let path = req.uri().path();

    match (method, path) {
        // GET /hello
        (&Method::GET, "/hello") => {
            let count = state.counter.fetch_add(1, Ordering::Relaxed);
            json_response(
                StatusCode::OK,
                &serde_json::json!({ "message": "Hello!", "count": count }),
            )
        }

        // POST /echo — retorna o body como JSON
        (&Method::POST, "/echo") => {
            let bytes = match hyper::body::to_bytes(req.into_body()).await {
                Ok(b) => b,
                Err(_) => return error(StatusCode::BAD_REQUEST, "invalid body"),
            };
            let payload: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(_) => return error(StatusCode::BAD_REQUEST, "invalid json"),
            };
            json_response(StatusCode::OK, &payload)
        }

        // GET /health
        (&Method::GET, "/health") => {
            Response::new(String::new())
        }

        _ => error(StatusCode::NOT_FOUND, "not found"),
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let state = AppState {
        counter: AtomicU64::new(0),
    };

    let addr: SocketAddr = ([0, 0, 0, 0], 3000).into();
    let listener = TcpListener::bind(addr).await.unwrap();

    eprintln!("Listening on http://{}", addr);

    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            let svc = service_fn(move |req| async {
                Ok::<_, hyper::Error>(handle(req, &state))
            });

            if let Err(err) = http1::Builder::new()
                .serve_connection(io, svc)
                .await
            {
                eprintln!("conn error: {}", err);
            }
        });
    }
}
