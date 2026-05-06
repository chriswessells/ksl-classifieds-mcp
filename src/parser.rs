use scraper::{Html, Selector};
use tracing::{debug, warn};

use crate::{
    error::{KslError, Result},
    types::{Listing, ListingDetail, Platform},
};

pub fn parse_search_results(html: &str) -> Result<Vec<Listing>> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse(r#"a[role="listitem"][href*="/listing/"]"#).unwrap();
    let price_sel = Selector::parse(r#"div[aria-label^="Price"]"#).unwrap();
    let img_sel = Selector::parse("img").unwrap();
    let loc_sel = Selector::parse(r#"span[role="link"]"#).unwrap();
    let fav_sel = Selector::parse("span.text-sm").unwrap();

    let mut listings = Vec::new();

    for el in doc.select(&sel) {
        let href = match el.value().attr("href") {
            Some(h) => h,
            None => {
                debug!("listing element missing href, skipping");
                continue;
            }
        };

        let id = match href.rsplit('/').next().filter(|s| !s.is_empty()) {
            Some(id) => id.to_string(),
            None => {
                debug!("could not extract id from href {}, skipping", href);
                continue;
            }
        };

        let title = el.value().attr("aria-label").unwrap_or("").to_string();

        // Price: div[aria-label="Price $X.XX"]
        let price = el.select(&price_sel).next().and_then(|p| {
            let label = p.value().attr("aria-label")?;
            // "Price $489.00" → parse after '$'
            let dollar_pos = label.find('$')?;
            label[dollar_pos + 1..].parse::<f64>().ok()
        });

        // Location: span[role="link"] contains "City<!-- -->, <!-- -->State"
        let (city, state) = el
            .select(&loc_sel)
            .next()
            .map(|loc| {
                // Get text nodes, stripping HTML comments
                let text: String = loc.text().collect();
                // text looks like "Midvale, UT" after comment stripping
                let parts: Vec<&str> = text.splitn(2, ',').collect();
                let city = parts.first().map(|s| s.trim().to_string());
                let state = parts.get(1).map(|s| s.trim().to_string());
                (city, state)
            })
            .unwrap_or((None, None));

        // Image
        let image_url = el
            .select(&img_sel)
            .next()
            .and_then(|img| img.value().attr("src"))
            .map(|s| s.to_string());

        // Favorites: span.text-sm containing a number (after heart button)
        let favorites_count = el.select(&fav_sel).find_map(|span| {
            let text = span.text().collect::<String>();
            text.trim().parse::<u32>().ok()
        });

        listings.push(Listing {
            id,
            title,
            price,
            city,
            state,
            url: href.to_string(),
            image_url,
            category: None,
            favorites_count,
            platform: Platform::Classifieds,
        });
    }

    if listings.is_empty() && html.len() > 5000 {
        warn!(
            "parsed 0 listings from {}B response — possible selector mismatch",
            html.len()
        );
    }

    Ok(listings)
}

pub fn has_more_results(listings: &[Listing], per_page: u32) -> bool {
    listings.len() >= per_page as usize
}

pub fn parse_listing_detail(html: &str, id: &str, url: &str) -> Result<ListingDetail> {
    // Primary: extract window.detailPage.listingData JSON from script tags
    if let Some(detail) = try_parse_from_script(html, id, url) {
        return Ok(detail);
    }
    // Fallback: CSS selector scraping
    parse_listing_detail_fallback(html, id, url)
}

fn try_parse_from_script(html: &str, id: &str, url: &str) -> Option<ListingDetail> {
    let needle = "listingData";
    let start = html.find(needle)?;
    // Find the '=' after the needle
    let eq_pos = html[start..].find('=')? + start;
    let after_eq = html[eq_pos + 1..].trim_start();
    // Find the JSON object boundaries
    let obj_start = after_eq.find('{')? ;
    let json_str = &after_eq[obj_start..];
    // Find matching closing brace
    let json_end = find_json_end(json_str)?;
    let json_str = &json_str[..json_end];

    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let title = v["title"].as_str().unwrap_or("").to_string();
    if title.is_empty() {
        return None;
    }

    let description = v["description"].as_str().map(str::to_string);
    let price = v["price"].as_f64().or_else(|| {
        v["price"].as_str().and_then(|s| s.replace(',', "").parse().ok())
    });
    let city = v["city"].as_str().map(str::to_string);
    let state = v["state"].as_str().map(str::to_string);
    let seller_type = v["sellerType"].as_str().map(str::to_string);
    let posted_date = v["createTime"].as_str()
        .or_else(|| v["postedDate"].as_str())
        .map(str::to_string);
    let condition = v["newUsed"].as_str().map(str::to_string);

    let photos = v["photos"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    p.as_str()
                        .or_else(|| p["url"].as_str())
                        .map(normalize_photo_url)
                })
                .collect()
        })
        .unwrap_or_default();

    Some(ListingDetail {
        id: id.to_string(),
        title,
        description,
        price,
        photos,
        city,
        state,
        seller_type,
        posted_date,
        condition,
        url: url.to_string(),
        platform: Platform::Classifieds,
    })
}

fn find_json_end(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (i, c) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_listing_detail_fallback(html: &str, id: &str, url: &str) -> Result<ListingDetail> {
    let doc = Html::parse_document(html);

    let title = {
        let sel = Selector::parse(r#"[data-testid="listing-title"], h1"#).unwrap();
        doc.select(&sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| KslError::Parse {
                context: format!("could not find title for listing {}", id),
            })?
    };

    let description = {
        let sel = Selector::parse(r#"[data-testid="listing-description"], .description"#).unwrap();
        doc.select(&sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let price = {
        let sel = Selector::parse("*").unwrap();
        doc.select(&sel).find_map(|e| {
            let text: String = e.text().collect();
            let text = text.trim();
            text.strip_prefix('$')
                .and_then(|s| s.replace(',', "").parse::<f64>().ok())
        })
    };

    let photos = {
        let sel = Selector::parse(r#"img[src*="ksldigital.com"], img[src*="ksl.com"]"#).unwrap();
        doc.select(&sel)
            .filter_map(|img| img.value().attr("src").map(normalize_photo_url))
            .collect()
    };

    Ok(ListingDetail {
        id: id.to_string(),
        title,
        description,
        price,
        photos,
        city: None,
        state: None,
        seller_type: None,
        posted_date: None,
        condition: None,
        url: url.to_string(),
        platform: Platform::Classifieds,
    })
}

fn normalize_photo_url(url: &str) -> String {
    if url.starts_with("//") {
        format!("https:{}", url)
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_fixture() {
        let html = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/search_results.html"),
        )
        .expect("fixture file missing");

        let listings = parse_search_results(&html).expect("parse failed");

        // CAPTURE_NOTES.md says ~10 listings
        assert!(!listings.is_empty(), "expected at least one listing");
        assert!(listings.len() >= 5, "expected at least 5 listings, got {}", listings.len());

        let first = &listings[0];
        assert!(!first.id.is_empty(), "id should not be empty");
        assert!(!first.title.is_empty(), "title should not be empty");
        assert!(!first.url.is_empty(), "url should not be empty");
        assert!(first.url.contains("/listing/"), "url should contain /listing/");

        // Known first listing from fixture
        assert_eq!(first.id, "78386739");
        assert_eq!(first.url, "https://classifieds.ksl.com/listing/78386739");
        assert!(first.price.is_some(), "first listing should have a price");
    }

    #[test]
    fn test_has_more_results() {
        let listings: Vec<Listing> = (0..24)
            .map(|i| Listing {
                id: i.to_string(),
                title: "t".into(),
                price: None,
                city: None,
                state: None,
                url: format!("https://classifieds.ksl.com/listing/{}", i),
                image_url: None,
                category: None,
                favorites_count: None,
                platform: Platform::Classifieds,
            })
            .collect();

        assert!(has_more_results(&listings, 24));
        assert!(!has_more_results(&listings[..10], 24));
    }

    #[test]
    fn test_parse_listing_detail_fixture() {
        let html = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/listing_detail.html"),
        )
        .expect("fixture file missing");

        let url = "https://classifieds.ksl.com/listing/12345678";
        let detail = parse_listing_detail(&html, "12345678", url).expect("parse failed");

        assert_eq!(detail.id, "12345678");
        assert!(!detail.title.is_empty(), "title should not be empty");
        assert_eq!(detail.url, url);
        assert_eq!(detail.platform, Platform::Classifieds);
    }

    #[test]
    fn test_normalize_photo_url() {
        assert_eq!(
            normalize_photo_url("//image.ksldigital.com/foo.jpg"),
            "https://image.ksldigital.com/foo.jpg"
        );
        assert_eq!(
            normalize_photo_url("https://image.ksldigital.com/foo.jpg"),
            "https://image.ksldigital.com/foo.jpg"
        );
    }
}
