use uuid::Uuid;

use crate::types::Listing;

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

pub fn render_report(listings: &[Listing], report_id: &Uuid, csrf_token: &str, port: u16) -> String {
    let mut items = String::new();
    for listing in listings {
        let title = escape_html(&listing.title);
        let price = listing.price.map(|p| format!("${p:.0}")).unwrap_or_else(|| "N/A".to_string());
        let location = match (&listing.city, &listing.state) {
            (Some(c), Some(s)) => format!("{}, {}", escape_html(c), escape_html(s)),
            (Some(c), None) => escape_html(c),
            _ => String::new(),
        };
        let id = escape_html(&listing.id);
        let url = escape_html(&listing.url);
        let img_tag = match &listing.image_url {
            Some(u) if !u.is_empty() => format!(
                r#"<img src="{}" alt="{}" style="width:150px;height:150px;object-fit:cover;" loading="lazy">"#,
                escape_html(u), title
            ),
            _ => r#"<div style="width:150px;height:150px;background:#eee;display:flex;align-items:center;justify-content:center;font-size:0.8em;">No Photo</div>"#.to_string(),
        };

        items.push_str(&format!(
            r#"<div style="border:1px solid #ddd;padding:12px;margin:8px;display:inline-block;width:200px;vertical-align:top;box-sizing:border-box;">
<label style="cursor:pointer;">
<input type="checkbox" name="selected" value="{id}" style="margin-bottom:6px;">
{img_tag}
<div style="font-weight:bold;margin-top:6px;font-size:0.9em;">{title}</div>
<div style="color:#333;">{price}</div>
<div style="color:#666;font-size:0.85em;">{location}</div>
<a href="{url}" target="_blank" rel="noopener noreferrer" style="font-size:0.8em;">View on KSL</a>
</label>
</div>
"#
        ));
    }

    let count = listings.len();
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>KSL Search Results</title>
</head>
<body style="font-family:system-ui,sans-serif;max-width:1400px;margin:0 auto;padding:20px;">
<h1>KSL Search Results ({count} listings)</h1>
<form method="POST" action="http://127.0.0.1:{port}/report/{report_id}/submit">
<input type="hidden" name="csrf_token" value="{csrf_token}">
<div style="margin-bottom:16px;">
<button type="submit" style="padding:10px 24px;font-size:1em;cursor:pointer;background:#0066cc;color:#fff;border:none;border-radius:4px;">Save Selected Items</button>
</div>
{items}
<div style="margin-top:16px;">
<button type="submit" style="padding:10px 24px;font-size:1em;cursor:pointer;background:#0066cc;color:#fff;border:none;border-radius:4px;">Save Selected Items</button>
</div>
</form>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::types::{Listing, Platform};

    fn xss_listing() -> Listing {
        Listing {
            id: "123".to_string(),
            title: "<script>alert(1)</script>".to_string(),
            price: Some(100.0),
            city: Some("<b>City</b>".to_string()),
            state: Some("UT".to_string()),
            url: "https://ksl.com/listing/123".to_string(),
            image_url: Some("https://image.ksldigital.com/img.jpg".to_string()),
            category: None,
            favorites_count: None,
            platform: Platform::Classifieds,
        }
    }

    #[test]
    fn test_escape_html_xss_title() {
        let escaped = escape_html("<script>alert(1)</script>");
        assert!(!escaped.contains('<'));
        assert!(!escaped.contains('>'));
        assert!(escaped.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_escape_html_all_five_chars() {
        let input = r#"& < > " '"#;
        let out = escape_html(input);
        assert!(out.contains("&amp;"));
        assert!(out.contains("&lt;"));
        assert!(out.contains("&gt;"));
        assert!(out.contains("&quot;"));
        assert!(out.contains("&#x27;"));
    }

    #[test]
    fn test_render_report_escapes_xss_in_title() {
        let id = Uuid::new_v4();
        let html = render_report(&[xss_listing()], &id, "tok", 12345);
        assert!(!html.contains("<script>alert(1)</script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_render_report_escapes_xss_in_city() {
        let id = Uuid::new_v4();
        let html = render_report(&[xss_listing()], &id, "tok", 12345);
        assert!(!html.contains("<b>City</b>"));
        assert!(html.contains("&lt;b&gt;City&lt;/b&gt;"));
    }

    #[test]
    fn test_render_report_contains_csrf_token() {
        let id = Uuid::new_v4();
        let html = render_report(&[xss_listing()], &id, "mycsrftoken", 12345);
        assert!(html.contains(r#"value="mycsrftoken""#));
    }

    #[test]
    fn test_render_report_csrf_not_in_url() {
        let id = Uuid::new_v4();
        let html = render_report(&[xss_listing()], &id, "mycsrftoken", 12345);
        // CSRF must be in hidden field, not in the action URL
        let action_line = html.lines().find(|l| l.contains("action=")).unwrap_or("");
        assert!(!action_line.contains("mycsrftoken"));
    }
}
