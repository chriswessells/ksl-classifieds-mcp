pub mod template;

use axum::{
    Router,
    extract::{Path, RawForm, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rand::RngCore;
use std::{collections::HashMap, sync::{Arc, Mutex}};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::types::Listing;

#[derive(Clone)]
pub struct ReportServer {
    state: Arc<ReportState>,
}

struct ReportState {
    port: Mutex<Option<u16>>,
    active_report: Mutex<Option<ActiveReport>>,
    pending_selections: Mutex<HashMap<Uuid, Vec<String>>>,
}

struct ActiveReport {
    id: Uuid,
    html: String,
    csrf_token: String,
}

impl ReportServer {
    pub fn new() -> Self {
        Self {
            state: Arc::new(ReportState {
                port: Mutex::new(None),
                active_report: Mutex::new(None),
                pending_selections: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub async fn ensure_started(&self) -> Result<u16, String> {
        if let Some(port) = *self.state.port.lock().unwrap() {
            return Ok(port);
        }
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind report server: {e}"))?;
        let port = listener.local_addr().unwrap().port();
        let state = self.state.clone();
        let app: Router = Router::new()
            .route("/report/:id", get(serve_report))
            .route("/report/:id/submit", post(handle_submit))
            .with_state(state);
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        *self.state.port.lock().unwrap() = Some(port);
        Ok(port)
    }

    pub fn register_report(&self, listings: &[Listing]) -> (String, Uuid) {
        let id = Uuid::new_v4();
        let csrf_token = generate_csrf_token();
        let port = self.state.port.lock().unwrap().expect("server must be started before register_report");
        let html = template::render_report(listings, &id, &csrf_token, port);
        *self.state.active_report.lock().unwrap() = Some(ActiveReport { id, html, csrf_token });
        (format!("http://127.0.0.1:{port}/report/{id}"), id)
    }

    pub fn take_pending_selections(&self) -> Option<(Uuid, Vec<String>)> {
        let mut map = self.state.pending_selections.lock().unwrap();
        let key = *map.keys().next()?;
        let val = map.remove(&key).unwrap();
        Some((key, val))
    }
}

fn generate_csrf_token() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn make_response(status: StatusCode, content_type: &'static str, body: String) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("content-security-policy", HeaderValue::from_static(
        "default-src 'self'; img-src https://image.ksldigital.com https://img.ksl.com; script-src 'none'; style-src 'unsafe-inline'"
    ));
    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("content-type", HeaderValue::from_static(content_type));
    (status, headers, body).into_response()
}

async fn serve_report(
    Path(id): Path<Uuid>,
    State(state): State<Arc<ReportState>>,
) -> Response {
    let html = {
        let guard = state.active_report.lock().unwrap();
        match &*guard {
            Some(r) if r.id == id => Some(r.html.clone()),
            _ => None,
        }
    };
    match html {
        Some(body) => make_response(StatusCode::OK, "text/html; charset=utf-8", body),
        None => make_response(StatusCode::NOT_FOUND, "text/plain", "Report not found or expired.".to_string()),
    }
}

async fn handle_submit(
    Path(id): Path<Uuid>,
    State(state): State<Arc<ReportState>>,
    RawForm(body): RawForm,
) -> Response {
    // Parse form body manually to support repeated `selected` keys
    let pairs: Vec<(String, String)> = form_urlencoded::parse(&body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let csrf_token = pairs.iter()
        .find(|(k, _)| k == "csrf_token")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");

    let selected: Vec<String> = pairs.iter()
        .filter(|(k, _)| k == "selected")
        .map(|(_, v)| v.clone())
        .collect();

    let valid = {
        let guard = state.active_report.lock().unwrap();
        matches!(&*guard, Some(r) if r.id == id && r.csrf_token == csrf_token)
    };
    if !valid {
        return make_response(StatusCode::FORBIDDEN, "text/plain", "Invalid or expired CSRF token.".to_string());
    }
    state.pending_selections.lock().unwrap().insert(id, selected);
    *state.active_report.lock().unwrap() = None;
    make_response(
        StatusCode::OK,
        "text/html; charset=utf-8",
        "<html><body><h1>Selections saved!</h1><p>Return to your AI assistant to continue.</p></body></html>".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Listing, Platform};

    fn make_listing(id: &str, title: &str) -> Listing {
        Listing {
            id: id.to_string(),
            title: title.to_string(),
            price: Some(50.0),
            city: None,
            state: None,
            url: format!("https://ksl.com/listing/{id}"),
            image_url: None,
            category: None,
            favorites_count: None,
            platform: Platform::Classifieds,
        }
    }

    #[tokio::test]
    async fn test_server_starts_and_returns_port() {
        let server = ReportServer::new();
        let port = server.ensure_started().await.expect("server should start");
        assert!(port > 0);
        let port2 = server.ensure_started().await.expect("second call should succeed");
        assert_eq!(port, port2);
    }

    #[tokio::test]
    async fn test_csrf_invalid_token_returns_403() {
        let server = ReportServer::new();
        server.ensure_started().await.unwrap();
        let listings = vec![make_listing("1", "Test Item")];
        let (url, _id) = server.register_report(&listings);
        let submit_url = url + "/submit";

        let client = reqwest::Client::new();
        let resp = client
            .post(&submit_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body("csrf_token=wrongtoken&selected=1")
            .send()
            .await
            .expect("request should succeed");
        assert_eq!(resp.status(), 403);
    }

    #[tokio::test]
    async fn test_csrf_valid_token_returns_200_and_stores_selections() {
        let server = ReportServer::new();
        server.ensure_started().await.unwrap();
        let listings = vec![make_listing("42", "Good Item")];
        let (url, _id) = server.register_report(&listings);
        let submit_url = url.clone() + "/submit";

        let client = reqwest::Client::new();
        let html = client.get(&url).send().await.unwrap().text().await.unwrap();
        let csrf = html
            .lines()
            .find(|l| l.contains("csrf_token") && l.contains("value="))
            .and_then(|l| l.split("value=\"").nth(1))
            .and_then(|s| s.split('"').next())
            .expect("csrf token must be in HTML");

        let body = format!("csrf_token={csrf}&selected=42");
        let resp = client
            .post(&submit_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let pending = server.take_pending_selections();
        assert!(pending.is_some());
        let (_rid, ids) = pending.unwrap();
        assert!(ids.contains(&"42".to_string()));
    }

    #[tokio::test]
    async fn test_csrf_multiple_selections() {
        let server = ReportServer::new();
        server.ensure_started().await.unwrap();
        let listings = vec![
            make_listing("1", "Item One"),
            make_listing("2", "Item Two"),
        ];
        let (url, _id) = server.register_report(&listings);
        let submit_url = url.clone() + "/submit";

        let client = reqwest::Client::new();
        let html = client.get(&url).send().await.unwrap().text().await.unwrap();
        let csrf = html
            .lines()
            .find(|l| l.contains("csrf_token") && l.contains("value="))
            .and_then(|l| l.split("value=\"").nth(1))
            .and_then(|s| s.split('"').next())
            .expect("csrf token must be in HTML");

        let body = format!("csrf_token={csrf}&selected=1&selected=2");
        let resp = client
            .post(&submit_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let pending = server.take_pending_selections();
        let (_rid, ids) = pending.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"1".to_string()));
        assert!(ids.contains(&"2".to_string()));
    }

    #[tokio::test]
    async fn test_security_headers_present() {
        let server = ReportServer::new();
        server.ensure_started().await.unwrap();
        let listings = vec![make_listing("1", "Item")];
        let (url, _) = server.register_report(&listings);

        let resp = reqwest::get(&url).await.unwrap();
        let headers = resp.headers();
        assert!(headers.contains_key("content-security-policy"));
        assert!(headers.contains_key("x-content-type-options"));
        assert!(headers.contains_key("x-frame-options"));
    }

    #[tokio::test]
    async fn test_old_report_returns_404_after_new_report_registered() {
        let server = ReportServer::new();
        server.ensure_started().await.unwrap();
        let listings = vec![make_listing("1", "Old Item")];
        let (old_url, _) = server.register_report(&listings);

        let (_, _) = server.register_report(&listings);

        let resp = reqwest::get(&old_url).await.unwrap();
        assert_eq!(resp.status(), 404);
    }
}
