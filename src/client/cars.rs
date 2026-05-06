use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use scraper::{Html, Selector};
use tracing::warn;

use crate::{
    client::rate_limiter::RateLimiter,
    config::Config,
    error::{KslError, Result},
    types::{CarListing, CarsSearchParams, CarsSearchResults, Platform},
};

const CARS_BASE_URL: &str = "https://cars.ksl.com/search";

#[derive(Clone)]
pub struct CarsClient {
    http: reqwest::Client,
    rate_limiter: RateLimiter,
}

impl CarsClient {
    pub fn new(config: &Config) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml"),
        );

        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .user_agent(&config.user_agent)
            .default_headers(headers)
            .build()
            .expect("failed to build cars HTTP client");

        CarsClient {
            http,
            rate_limiter: RateLimiter::new(config),
        }
    }

    pub async fn search_cars(&self, params: &CarsSearchParams) -> Result<CarsSearchResults> {
        self.rate_limiter.acquire().await?;

        let page = params.page.unwrap_or(1);
        let url = build_search_url(params);

        let mut retries = 0u8;
        loop {
            let resp = self.http.get(&url).send().await;

            match resp {
                Err(e) if retries < 2 => {
                    retries += 1;
                    warn!("Cars request error (retry {}): {}", retries, e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                Err(e) => return Err(KslError::Http(e)),
                Ok(r) => {
                    let status = r.status();

                    if status == 429 {
                        self.rate_limiter.record_failure();
                        return Err(KslError::RateLimited {
                            reason: "429 from cars server".into(),
                        });
                    }

                    if !status.is_success() {
                        if (status == 503 || status == 502 || status == 504) && retries < 2 {
                            retries += 1;
                            warn!("Transient {} (retry {})", status, retries);
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                        return Err(KslError::Parse {
                            context: format!("cars site returned {}", status),
                        });
                    }

                    let html = r.text().await.map_err(|e| KslError::Parse {
                        context: format!("cars response read: {}", e),
                    })?;

                    let listings = parse_car_listings(&html)?;
                    let has_more = listings.len() >= 24;

                    self.rate_limiter.record_success();

                    return Ok(CarsSearchResults {
                        listings,
                        page,
                        has_more,
                    });
                }
            }
        }
    }
}

fn build_search_url(params: &CarsSearchParams) -> String {
    let mut segments: Vec<String> = Vec::new();

    macro_rules! push_str {
        ($key:expr, $opt:expr) => {
            if let Some(v) = &$opt {
                segments.push(format!("{}/{}", $key, v));
            }
        };
    }
    macro_rules! push_num {
        ($key:expr, $opt:expr) => {
            if let Some(v) = $opt {
                segments.push(format!("{}/{}", $key, v));
            }
        };
    }

    push_str!("keyword", params.keyword);
    push_str!("make", params.make);
    push_str!("model", params.model);
    push_num!("yearFrom", params.year_from);
    push_num!("yearTo", params.year_to);
    push_num!("priceFrom", params.price_from);
    push_num!("priceTo", params.price_to);
    push_num!("mileageFrom", params.mileage_from);
    push_num!("mileageTo", params.mileage_to);
    push_str!("zip", params.zip);
    push_num!("miles", params.miles);
    push_str!("titleType", params.title_type);
    push_str!("drive", params.drive);
    push_str!("fuel", params.fuel);

    // KSL Cars uses 0-indexed pages
    let page = params.page.unwrap_or(1).saturating_sub(1);
    segments.push(format!("page/{}", page));

    if segments.is_empty() {
        CARS_BASE_URL.to_string()
    } else {
        format!("{}/{}", CARS_BASE_URL, segments.join("/"))
    }
}

fn parse_car_listings(html: &str) -> Result<Vec<CarListing>> {
    let doc = Html::parse_document(html);

    let listing_sel =
        Selector::parse(r#"a[role="listitem"][href*="cars.ksl.com/listing/"]"#).unwrap();
    let price_sel = Selector::parse(r#"[aria-label^="Price"]"#).unwrap();
    let location_sel = Selector::parse(r#"[role="link"]"#).unwrap();

    let mut listings = Vec::new();

    for el in doc.select(&listing_sel) {
        let href = el.value().attr("href").unwrap_or_default();
        let id = href
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();

        if id.is_empty() {
            continue;
        }

        let title = el
            .value()
            .attr("aria-label")
            .unwrap_or_default()
            .trim()
            .to_string();

        // Parse year/make/model from title (e.g. "1970 Volkswagen Beetle")
        let (year, make, model) = parse_title_parts(&title);

        // Price from aria-label="Price $X,XXX"
        let price = el.select(&price_sel).next().and_then(|p| {
            p.value()
                .attr("aria-label")
                .and_then(|l| l.strip_prefix("Price "))
                .map(|s| s.replace(['$', ','], ""))
                .and_then(|s| s.parse::<f64>().ok())
        });

        // Mileage from <span>X Miles</span>
        let mileage = el.text().find_map(|t| {
            let t = t.trim();
            t.strip_suffix(" Miles")
                .map(|m| m.replace(',', ""))
                .and_then(|m| m.parse::<u32>().ok())
        });

        // Location from role="link" span containing "City, ST"
        let (city, state) = el
            .select(&location_sel)
            .find_map(|loc| {
                let text: String = loc.text().collect();
                let text = text.trim().to_string();
                if text.contains(", ") {
                    Some(text)
                } else {
                    None
                }
            })
            .map(|loc| {
                let parts: Vec<&str> = loc.rsplitn(2, ", ").collect();
                if parts.len() == 2 {
                    (Some(parts[1].to_string()), Some(parts[0].to_string()))
                } else {
                    (Some(loc), None)
                }
            })
            .unwrap_or((None, None));

        // Photo from img alt matching title
        let photo_url = el
            .select(&Selector::parse("img").unwrap())
            .next()
            .and_then(|img| img.value().attr("src").map(String::from));

        let url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("https://cars.ksl.com{}", href)
        };

        listings.push(CarListing {
            id,
            title,
            price,
            make,
            model,
            year,
            mileage,
            city,
            state,
            zip: None,
            photo_url,
            description: None,
            seller_type: None,
            url,
            platform: Platform::Cars,
        });
    }

    Ok(listings)
}

fn parse_title_parts(title: &str) -> (Option<u32>, Option<String>, Option<String>) {
    let parts: Vec<&str> = title.splitn(3, ' ').collect();
    match parts.len() {
        3 => (
            parts[0].parse().ok(),
            Some(parts[1].to_string()),
            Some(parts[2].to_string()),
        ),
        2 => (parts[0].parse().ok(), Some(parts[1].to_string()), None),
        _ => (None, None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_search_url() {
        let params = CarsSearchParams {
            make: Some("Volkswagen".into()),
            year_from: Some(1950),
            year_to: Some(1973),
            page: Some(1),
            ..Default::default()
        };
        let url = build_search_url(&params);
        assert_eq!(
            url,
            "https://cars.ksl.com/search/make/Volkswagen/yearFrom/1950/yearTo/1973/page/0"
        );
    }

    #[test]
    fn test_build_search_url_page_2() {
        let params = CarsSearchParams {
            make: Some("Ford".into()),
            page: Some(2),
            ..Default::default()
        };
        let url = build_search_url(&params);
        assert_eq!(url, "https://cars.ksl.com/search/make/Ford/page/1");
    }

    #[test]
    fn test_parse_title_parts() {
        let (year, make, model) = parse_title_parts("1970 Volkswagen Beetle");
        assert_eq!(year, Some(1970));
        assert_eq!(make.as_deref(), Some("Volkswagen"));
        assert_eq!(model.as_deref(), Some("Beetle"));
    }

    #[test]
    fn test_parse_title_parts_with_trim() {
        let (year, make, model) = parse_title_parts("1967 Volkswagen Beetle 60s Edition");
        assert_eq!(year, Some(1967));
        assert_eq!(make.as_deref(), Some("Volkswagen"));
        assert_eq!(model.as_deref(), Some("Beetle 60s Edition"));
    }

    #[test]
    fn test_parse_car_listings_from_fixture() {
        // Test with a minimal HTML fixture
        let html = r#"
        <div role="list" aria-label="Search results">
            <a class="group" aria-label="1970 Volkswagen Beetle" href="https://cars.ksl.com/listing/10574046" role="listitem">
                <img alt="1970 Volkswagen Beetle" src="https://image.ksldigital.com/test.jpg" />
                <span>39,034 Miles</span>
                <span class="text-ksl-blue-500" role="link">Provo, UT</span>
                <div aria-label="Price $7,000">$7,000</div>
            </a>
        </div>
        "#;
        let listings = parse_car_listings(html).unwrap();
        assert_eq!(listings.len(), 1);
        let l = &listings[0];
        assert_eq!(l.id, "10574046");
        assert_eq!(l.title, "1970 Volkswagen Beetle");
        assert_eq!(l.price, Some(7000.0));
        assert_eq!(l.mileage, Some(39034));
        assert_eq!(l.city.as_deref(), Some("Provo"));
        assert_eq!(l.state.as_deref(), Some("UT"));
        assert_eq!(l.year, Some(1970));
        assert_eq!(l.make.as_deref(), Some("Volkswagen"));
        assert_eq!(l.model.as_deref(), Some("Beetle"));
        assert_eq!(l.platform, Platform::Cars);
    }
}
