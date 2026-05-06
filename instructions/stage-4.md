# Stage 4: Interactive Reports — Implementation Instructions

## 1. Update `Cargo.toml`

Add to `[dependencies]`:
```toml
axum = "0.7"
uuid = { version = "1", features = ["v4"] }
```

`tokio` already has `features = ["full"]` which includes `net`. No change needed there.

## 2. Create `src/report/mod.rs` — Singleton Axum Server

```rust
pub mod template;

use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Form,
};
use rand::RngCore;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::types::Listing;

#[derive(Clone)]
pub struct ReportServer {
    state: Arc<ReportState>,
}

struct ReportState {
    /// The port the server is listening on (set after bind)
    port: Mutex<Option<u16>>,
    /// Active report: (report_id, html_content, csrf_token)
    active_report: Mutex<Option<ActiveReport>>,
    /// Pending selections from submitted forms: report_id -> Vec<listing_id>
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

    /// Start the server if not already running. Returns the port.
    /// Fails within 500ms if bind fails.
    pub async fn ensure_started(&self) -> Result<u16, String> {
        // Check if already started
        if let Some(port) = *self.state.port.lock().unwrap() {
            return Ok(port);
        }

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind report server: {e}"))?;
        let port = listener.local_addr().unwrap().port();

        let state = self.state.clone();
        let app = Router::new()
            .route("/report/{id}", get(serve_report))
            .route("/report/{id}/submit", post(handle_submit))
            .with_state(state);

        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        *self.state.port.lock().unwrap() = Some(port);
        Ok(port)
    }

    /// Register a new report. Invalidates any previous active report.
    /// Returns (report_url, report_id).
    pub fn register_report(&self, listings: &[Listing]) -> (String, Uuid) {
        let id = Uuid::new_v4();
        let csrf_token = generate_csrf_token();
        let port = self.state.port.lock().unwrap().expect("server must be started");

        let html = template::render_report(listings, &id, &csrf_token, port);

        *self.state.active_report.lock().unwrap() = Some(ActiveReport {
            id,
            html,
            csrf_token,
        });

        let url = format!("http://127.0.0.1:{port}/report/{id}");
        (url, id)
    }

    /// Get pending selections for a report_id, removing them.
    pub fn take_pending_selections(&self, report_id: &Uuid) -> Option<Vec<String>> {
        self.state.pending_selections.lock().unwrap().remove(report_id)
    }

    /// Check if there are pending selections for any report.
    pub fn get_pending_selections(&self) -> Option<(Uuid, Vec<String>)> {
        let mut map = self.state.pending_selections.lock().unwrap();
        if map.is_empty() {
            return None;
        }
        // Return the first (and typically only) entry
        let key = *map.keys().next().unwrap();
        let val = map.remove(&key).unwrap();
        Some((key, val))
    }
}

fn generate_csrf_token() -> String {
    let mut bytes = [0u8; 16]; // 128-bit
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn security_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; img-src https://image.ksldigital.com https://img.ksl.com; script-src 'none'; style-src 'unsafe-inline'"
        ),
    );
    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers
}

async fn serve_report(
    Path(id): Path<Uuid>,
    State(state): State<Arc<ReportState>>,
) -> impl IntoResponse {
    let guard = state.active_report.lock().unwrap();
    match &*guard {
        Some(report) if report.id == id => {
            let mut headers = security_headers();
            headers.insert("content-type", HeaderValue::from_static("text/html; charset=utf-8"));
            (StatusCode::OK, headers, report.html.clone())
        }
        _ => {
            let headers = security_headers();
            (StatusCode::NOT_FOUND, headers, "Report not found or expired.".to_string())
        }
    }
}

#[derive(Deserialize)]
struct SubmitForm {
    csrf_token: String,
    #[serde(default)]
    selected: Vec<String>,
}

async fn handle_submit(
    Path(id): Path<Uuid>,
    State(state): State<Arc<ReportState>>,
    Form(form): Form<SubmitForm>,
) -> impl IntoResponse {
    // Validate CSRF
    let valid = {
        let guard = state.active_report.lock().unwrap();
        match &*guard {
            Some(report) if report.id == id => report.csrf_token == form.csrf_token,
            _ => false,
        }
    };

    if !valid {
        return (StatusCode::FORBIDDEN, security_headers(), "Invalid or expired CSRF token.".to_string());
    }

    // Store selections
    state.pending_selections.lock().unwrap().insert(id, form.selected);

    // Invalidate the report (single-use CSRF)
    *state.active_report.lock().unwrap() = None;

    let mut headers = security_headers();
    headers.insert("content-type", HeaderValue::from_static("text/html; charset=utf-8"));
    (StatusCode::OK, headers, "<html><body><h1>Selections saved!</h1><p>Return to your AI assistant to continue.</p></body></html>".to_string())
}

use serde::Deserialize;
```

**Key points:**
- `ReportServer` is `Clone` (wraps `Arc<ReportState>`)
- `ensure_started()` is idempotent — only binds once
- `register_report()` replaces any previous active report (one at a time)
- CSRF token is 128-bit from `OsRng`, hex-encoded (32 chars)
- CSRF is single-use: report is invalidated after successful submit
- Security headers on every response

## 3. Create `src/report/template.rs` — HTML Generation

```rust
use uuid::Uuid;
use crate::types::Listing;

/// Escape HTML special characters in user-supplied data.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Render the full HTML report page. All listing data is HTML-escaped.
pub fn render_report(listings: &[Listing], report_id: &Uuid, csrf_token: &str, port: u16) -> String {
    let mut items_html = String::new();
    for listing in listings {
        let title = escape_html(&listing.title);
        let price = listing.price.map(|p| format!("${:.2}", p)).unwrap_or_else(|| "N/A".to_string());
        let location = match (&listing.city, &listing.state) {
            (Some(c), Some(s)) => format!("{}, {}", escape_html(c), escape_html(s)),
            (Some(c), None) => escape_html(c),
            _ => String::new(),
        };
        let img = listing.image_url.as_deref().unwrap_or("");
        let img_tag = if img.is_empty() {
            r#"<div style="width:150px;height:150px;background:#eee;display:flex;align-items:center;justify-content:center;">No Photo</div>"#.to_string()
        } else {
            format!(r#"<img src="{}" alt="{}" style="width:150px;height:150px;object-fit:cover;" loading="lazy">"#, escape_html(img), title)
        };
        let id = escape_html(&listing.id);
        let url = escape_html(&listing.url);

        items_html.push_str(&format!(
            r#"<div style="border:1px solid #ddd;padding:12px;margin:8px;display:inline-block;width:200px;vertical-align:top;">
  <label>
    <input type="checkbox" name="selected" value="{id}">
    {img_tag}
    <div style="font-weight:bold;margin-top:8px;">{title}</div>
    <div>{price}</div>
    <div style="color:#666;font-size:0.9em;">{location}</div>
    <a href="{url}" target="_blank" rel="noopener" style="font-size:0.8em;">View on KSL</a>
  </label>
</div>"#
        ));
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>KSL Search Results</title>
</head>
<body style="font-family:system-ui,sans-serif;max-width:1200px;margin:0 auto;padding:20px;">
<h1>Search Results ({count} listings)</h1>
<form method="POST" action="http://127.0.0.1:{port}/report/{report_id}/submit">
  <input type="hidden" name="csrf_token" value="{csrf_token}">
  <div style="margin-bottom:16px;">
    <button type="submit" style="padding:10px 20px;font-size:1.1em;cursor:pointer;">Track Selected Items</button>
  </div>
  <div>{items_html}</div>
  <div style="margin-top:16px;">
    <button type="submit" style="padding:10px 20px;font-size:1.1em;cursor:pointer;">Track Selected Items</button>
  </div>
</form>
</body>
</html>"#,
        count = listings.len(),
        port = port,
        report_id = report_id,
        csrf_token = csrf_token,
        items_html = items_html,
    )
}
```

**Key points:**
- `escape_html()` handles the 5 critical characters
- All listing fields (`title`, `city`, `state`, `id`, `url`, `image_url`) are escaped before insertion
- No JavaScript — CSP `script-src 'none'` is enforced
- Inline styles only (allowed by `style-src 'unsafe-inline'`)
- Image sources restricted to KSL domains by CSP

## 4. Create `src/tools/report.rs` — MCP Tool Handlers

```rust
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    client::classifieds::ClassifiedsClient,
    report::ReportServer,
    types::{ClassifiedsSearchParams, SortOrder},
};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrowseSearchResultsInput {
    /// Keyword to search for
    pub keyword: Option<String>,
    /// Category name
    pub category: Option<String>,
    /// Minimum price in dollars
    pub price_from: Option<u32>,
    /// Maximum price in dollars
    pub price_to: Option<u32>,
    /// ZIP code for radius search
    pub zip: Option<String>,
    /// Radius in miles
    pub miles: Option<u32>,
    /// Sort order: Newest, Oldest, PriceLow, PriceHigh
    pub sort: Option<String>,
    /// Page number (0-indexed)
    pub page: Option<u32>,
}

pub async fn browse_search_results(
    client: &ClassifiedsClient,
    report_server: &ReportServer,
    input: BrowseSearchResultsInput,
) -> String {
    // 1. Ensure report server is running
    let port = match report_server.ensure_started().await {
        Ok(p) => p,
        Err(e) => return format!(r#"{{"error":"Report server failed to start: {e}"}}"#),
    };

    // 2. Run search
    let sort = input.sort.as_deref().and_then(|s| match s {
        "Newest" => Some(SortOrder::Newest),
        "Oldest" => Some(SortOrder::Oldest),
        "PriceLow" => Some(SortOrder::PriceLow),
        "PriceHigh" => Some(SortOrder::PriceHigh),
        _ => None,
    });

    let params = ClassifiedsSearchParams {
        keyword: input.keyword,
        category: input.category,
        price_from: input.price_from,
        price_to: input.price_to,
        zip: input.zip,
        miles: input.miles,
        sort,
        page: input.page,
        ..Default::default()
    };

    let results = match client.search_classifieds(&params).await {
        Ok(r) => r,
        Err(e) => return format!(r#"{{"error":"Search failed: {e}"}}"#),
    };

    if results.listings.is_empty() {
        return r#"{"error":"No results found."}"#.to_string();
    }

    // 3. Register report and open browser
    let (url, report_id) = report_server.register_report(&results.listings);

    if let Err(e) = open_browser(&url) {
        return format!(r#"{{"error":"Failed to open browser: {e}", "url":"{url}"}}"#);
    }

    format!(
        r#"{{"ok":true,"report_id":"{report_id}","url":"{url}","listing_count":{count},"message":"Report opened in browser. User can select items and submit. Use get_pending_selections to check for submissions."}}"#,
        report_id = report_id,
        url = url,
        count = results.listings.len(),
    )
}

pub fn get_pending_selections(report_server: &ReportServer) -> String {
    match report_server.get_pending_selections() {
        Some((report_id, selections)) => {
            serde_json::json!({
                "report_id": report_id.to_string(),
                "selected_listing_ids": selections,
                "count": selections.len(),
            }).to_string()
        }
        None => r#"{"pending":false,"message":"No selections submitted yet."}"#.to_string(),
    }
}

fn open_browser(url: &str) -> Result<(), String> {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "linux") {
        "xdg-open"
    } else {
        return Err("Unsupported platform. Only macOS and Linux are supported.".to_string());
    };

    std::process::Command::new(cmd)
        .arg(url)
        .spawn()
        .map_err(|e| format!("{cmd} failed: {e}"))?;
    Ok(())
}
```

## 5. Update `src/tools/mod.rs`

```rust
pub mod report;
pub mod search;
pub mod tracking;
```

## 6. Update `src/tools/search.rs` — Add Report Server + New Tools

Add `report_server: ReportServer` field to `KslMcpServer`:

```rust
use crate::report::ReportServer;
use crate::tools::report::{BrowseSearchResultsInput, self as report_tools};

#[derive(Clone)]
pub struct KslMcpServer {
    classifieds_client: ClassifiedsClient,
    cars_client: CarsClient,
    db: Option<crate::tools::tracking::DbHandle>,
    report_server: ReportServer,
}

impl KslMcpServer {
    pub fn new(
        classifieds_client: ClassifiedsClient,
        cars_client: CarsClient,
        db: Option<crate::db::Db>,
        report_server: ReportServer,
    ) -> Self {
        let db = db.map(|d| std::sync::Arc::new(std::sync::Mutex::new(d)));
        Self { classifieds_client, cars_client, db, report_server }
    }
}
```

Add two new tool methods inside the `#[tool(tool_box)] impl KslMcpServer` block:

```rust
    /// Browse search results in an interactive HTML report
    #[tool(description = "Search KSL Classifieds and open an interactive HTML report in the browser with photos and checkboxes for selecting items to track.")]
    async fn browse_search_results(&self, #[tool(aggr)] input: BrowseSearchResultsInput) -> String {
        report_tools::browse_search_results(&self.classifieds_client, &self.report_server, input).await
    }

    /// Check for pending user selections from a browse report
    #[tool(description = "Check if the user has submitted item selections from a browse_search_results report.")]
    fn get_pending_selections(&self) -> String {
        report_tools::get_pending_selections(&self.report_server)
    }
```

## 7. Update `src/main.rs`

Add `mod report;` to the module declarations. Update server construction:

```rust
mod report;

// In main():
let report_server = report::ReportServer::new();
let server = KslMcpServer::new(classifieds_client, cars_client, db, report_server);
```

## 8. Add `hex` Dependency to `Cargo.toml`

```toml
hex = "0.4"
```

This is used for encoding the 128-bit CSRF token bytes to a hex string.

## File Checklist

| File | Action |
|------|--------|
| `Cargo.toml` | Add `axum = "0.7"`, `uuid = { version = "1", features = ["v4"] }`, `hex = "0.4"` |
| `src/report/mod.rs` | Create — singleton server, CSRF, state management |
| `src/report/template.rs` | Create — HTML render function with escape_html |
| `src/tools/report.rs` | Create — browse_search_results, get_pending_selections, open_browser |
| `src/tools/mod.rs` | Add `pub mod report;` |
| `src/tools/search.rs` | Add `report_server` field, update `new()`, add 2 tool methods |
| `src/main.rs` | Add `mod report;`, create `ReportServer`, pass to `KslMcpServer::new()` |

## Security Summary

| Concern | Mitigation |
|---------|-----------|
| XSS from listing data | `escape_html()` on all 5 chars; CSP `script-src 'none'` |
| CSRF | 128-bit token from `OsRng`, hidden form field, validated server-side, single-use |
| Network exposure | Binds `127.0.0.1:0` only (localhost, OS-assigned port) |
| Response headers | CSP + `X-Content-Type-Options: nosniff` + `X-Frame-Options: DENY` |
| Image loading | CSP `img-src` restricted to KSL image domains |

## Behavioral Notes

- **One active report at a time**: `register_report()` replaces the previous `ActiveReport`. Old report URLs return 404.
- **Lazy start**: Server only binds on first `browse_search_results` call. Zero overhead if never used.
- **Single-use CSRF**: After successful form submission, the active report is cleared. Resubmission returns 403.
- **Pending selections**: Stored in `HashMap<Uuid, Vec<String>>` behind `Arc<Mutex>`. Retrieved (and removed) by `get_pending_selections`.
- **Browser open**: Uses `Command::new("open")` on macOS, `Command::new("xdg-open")` on Linux. Returns error string on unsupported platforms.
- **Startup timeout**: `TcpListener::bind` is near-instant; if it fails, error propagates immediately (well within 500ms).
