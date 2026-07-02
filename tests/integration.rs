use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Shared compose stack — starts once for all tests
// ---------------------------------------------------------------------------

static COMPOSE: OnceLock<()> = OnceLock::new();

fn ensure_compose() {
    COMPOSE.get_or_init(|| {
        // clean up any leftover
        let _ = Command::new("docker")
            .args(["compose", "-f", "docker-compose.test.yml", "down", "-v"])
            .status();

        let status = Command::new("docker")
            .args(["compose", "-f", "docker-compose.test.yml", "up", "--build", "-d"])
            .status()
            .expect("failed to start docker compose");

        assert!(status.success(), "docker compose up failed");

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();

        for i in 0..30 {
            let Ok(resp) = client
                .get("http://localhost:8080/health-check")
                .send()
            else {
                eprintln!("Waiting for API... attempt {}", i + 1);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            if resp.status().is_success() {
                eprintln!("API is ready!");
                return ();
            }
            eprintln!("Waiting for API... attempt {} (status: {})", i + 1, resp.status());
            std::thread::sleep(Duration::from_secs(2));
        }
        panic!("API never became ready");
    });
}

fn get(path: &str) -> reqwest::blocking::Response {
    ensure_compose();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    client
        .get(&format!("http://localhost:8080{}", path))
        .send()
        .expect("GET request failed")
}

fn post(path: &str, body: &str) -> reqwest::blocking::Response {
    ensure_compose();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    client
        .post(&format!("http://localhost:8080{}", path))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .expect("POST request failed")
}

// Clean up after all tests
fn cleanup() {
    let _ = Command::new("docker")
        .args(["compose", "-f", "docker-compose.test.yml", "down", "-v"])
        .status();
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn unique_apelido(prefix: &str) -> String {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string();
    let max_prefix = 32usize.saturating_sub(suffix.len());
    let prefix = &prefix[..prefix.len().min(max_prefix)];
    format!("{}{}", prefix, suffix)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_health_check() {
    let resp = get("/health-check");
    assert_eq!(resp.status(), 200);
}

#[test]
fn test_create_person() {
    let apelido = unique_apelido("test");
    let body = format!(
        r#"{{"apelido":"{}","nome":"Test User","nascimento":"1990-01-01","stack":["Go","Rust"]}}"#,
        apelido
    );
    let resp = post("/pessoas", &body);
    assert_eq!(resp.status(), 201, "create failed: {:?}", resp.text());
    let json: serde_json::Value = resp.json().unwrap();
    assert!(json.get("id").and_then(|v| v.as_str()).is_some());

    // same nickname → conflict
    let resp2 = post("/pessoas", &body);
    assert_eq!(resp2.status(), 422);
}

#[test]
fn test_create_person_invalid() {
    // empty nickname
    let resp = post("/pessoas", r#"{"apelido":"","nome":"Test","nascimento":"1990-01-01"}"#);
    assert_eq!(resp.status(), 422);

    // field too long
    let resp = post(
        "/pessoas",
        r#"{"apelido":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","nome":"Test","nascimento":"1990-01-01"}"#,
    );
    assert_eq!(resp.status(), 422);

    // invalid json → 400
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let resp = client
        .post("http://localhost:8080/pessoas")
        .header("Content-Type", "application/json")
        .body("not json")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[test]
fn test_get_person_by_id() {
    let apelido = unique_apelido("get");
    let body = format!(
        r#"{{"apelido":"{}","nome":"Get Test","nascimento":"1990-01-01","stack":["Go"]}}"#,
        apelido
    );
    let resp = post("/pessoas", &body);
    assert_eq!(resp.status(), 201);
    let json: serde_json::Value = resp.json().unwrap();
    let id = json["id"].as_str().unwrap().to_string();

    // get by id
    let resp = get(&format!("/pessoas/{}", id));
    assert_eq!(resp.status(), 200);
    let get_body = resp.text().unwrap();
    assert!(get_body.contains(&id));

    // not found
    let resp = get("/pessoas/00000000-0000-0000-0000-000000000000");
    assert_eq!(resp.status(), 404);
}

#[test]
fn test_search() {
    let a1 = unique_apelido("s1");
    let a2 = unique_apelido("s2");
    post(
        "/pessoas",
        &format!(
            r#"{{"apelido":"{}","nome":"Search One","nascimento":"1990-01-01","stack":["Go"]}}"#,
            a1
        ),
    );
    post(
        "/pessoas",
        &format!(
            r#"{{"apelido":"{}","nome":"Search Two","nascimento":"1991-01-01","stack":["Rust"]}}"#,
            a2
        ),
    );

    // search by first 8 chars of a1 (the numeric suffix starts with the same digits)
    let resp = get(&format!("/pessoas?t={}", &a1[..a1.len().min(8)]));
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().unwrap();
    let people = body.as_array().unwrap();
    assert!(!people.is_empty(), "expected at least one result");

    // search without term → 400
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let resp = client
        .get("http://localhost:8080/pessoas")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[test]
fn test_count() {
    let resp = get("/contagem-pessoas");
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert!(json.get("count").is_some());
}
