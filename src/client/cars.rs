use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    client::rate_limiter::RateLimiter,
    config::Config,
    error::{KslError, Result},
    types::{CarListing, CarsSearchParams, CarsSearchResults, Platform},
};

const CARS_API_URL: &str = "https://cars.ksl.com/nextjs-api/proxy";

#[derive(Clone)]
pub struct CarsClient {
    http: reqwest::Client,
    rate_limiter: RateLimiter,
}

impl CarsClient {
    pub fn new(config: &Config) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://cars.ksl.com"),
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

        let per_page = params.per_page.unwrap_or(24);
        let page = params.page.unwrap_or(1);
        let body = build_request_body(params);

        let mut retries = 0u8;
        loop {
            let resp = self.http.post(CARS_API_URL).json(&body).send().await;

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
                            context: format!("cars API returned {}", status),
                        });
                    }

                    let api_resp: CarsApiResponse =
                        r.json().await.map_err(|e| KslError::Parse {
                            context: format!("cars JSON parse: {}", e),
                        })?;

                    let items = api_resp.data.items;
                    let has_more = items.len() >= per_page as usize;
                    let listings = items.into_iter().map(|item| item.into_listing()).collect();

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

fn build_request_body(params: &CarsSearchParams) -> Value {
    let mut arr: Vec<Value> = Vec::new();

    macro_rules! push_str {
        ($key:expr, $opt:expr) => {
            if let Some(v) = &$opt {
                arr.push(json!($key));
                arr.push(json!(v));
            }
        };
    }
    macro_rules! push_num {
        ($key:expr, $opt:expr) => {
            if let Some(v) = $opt {
                arr.push(json!($key));
                arr.push(json!(v));
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

    arr.push(json!("perPage"));
    arr.push(json!(params.per_page.unwrap_or(24)));
    arr.push(json!("page"));
    arr.push(json!(params.page.unwrap_or(1)));

    json!({
        "endpoint": "/classifieds/cars/search/searchByUrlParams",
        "options": {
            "method": "POST",
            "headers": {
                "Content-Type": "application/json",
                "User-Agent": "cars-node",
                "X-App-Source": "frontline",
                "X-DDM-EVENT-USER-AGENT": {},
                "X-DDM-EVENT-ACCEPT-LANGUAGE": "en-US",
                "X-MEMBER-ID": null,
                "cookie": ""
            },
            "body": arr
        }
    })
}

// Wire types for response deserialization
#[derive(Deserialize)]
struct CarsApiResponse {
    data: CarsApiData,
}

#[derive(Deserialize)]
struct CarsApiData {
    items: Vec<RawCarItem>,
}

#[derive(Deserialize)]
struct RawCarItem {
    id: Option<Value>,
    title: Option<String>,
    price: Option<f64>,
    make: Option<String>,
    model: Option<String>,
    year: Option<u32>,
    mileage: Option<u32>,
    city: Option<String>,
    state: Option<String>,
    zip: Option<String>,
    photo: Option<String>,
    description: Option<String>,
    #[serde(rename = "sellerType")]
    seller_type: Option<String>,
}

impl RawCarItem {
    fn into_listing(self) -> CarListing {
        let id = match &self.id {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let url = format!("https://cars.ksl.com/listing/{}", id);
        CarListing {
            id,
            title: self.title.unwrap_or_default(),
            price: self.price,
            make: self.make,
            model: self.model,
            year: self.year,
            mileage: self.mileage,
            city: self.city,
            state: self.state,
            zip: self.zip,
            photo_url: self.photo,
            description: self.description,
            seller_type: self.seller_type,
            url,
            platform: Platform::Cars,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_request_body_flat_array() {
        let params = CarsSearchParams {
            make: Some("Ford".into()),
            price_from: Some(2000),
            price_to: Some(5000),
            page: Some(1),
            per_page: Some(24),
            ..Default::default()
        };
        let body = build_request_body(&params);
        let arr = body["options"]["body"].as_array().unwrap();

        // Should be flat alternating key/value pairs
        assert!(arr.len().is_multiple_of(2));

        // Find "make" key and check value
        let pos = arr.iter().position(|v| v == "make").unwrap();
        assert_eq!(arr[pos + 1], "Ford");

        let pos = arr.iter().position(|v| v == "priceFrom").unwrap();
        assert_eq!(arr[pos + 1], 2000);

        let pos = arr.iter().position(|v| v == "page").unwrap();
        assert_eq!(arr[pos + 1], 1);
    }

    #[test]
    fn test_parse_cars_fixture() {
        let json_str = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/cars_search.json"),
        )
        .expect("fixture file missing");

        let resp: CarsApiResponse = serde_json::from_str(&json_str).expect("parse failed");
        assert!(!resp.data.items.is_empty());

        let listing = resp.data.items.into_iter().next().unwrap().into_listing();
        assert!(!listing.id.is_empty());
        assert!(listing.url.contains("cars.ksl.com/listing/"));
        assert_eq!(listing.platform, Platform::Cars);
    }
}
