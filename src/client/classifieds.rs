use std::time::Duration;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::header::{self, HeaderMap, HeaderValue};
use tracing::warn;

use crate::{
    client::{KslClient, rate_limiter::RateLimiter},
    config::Config,
    error::{KslError, Result},
    parser,
    types::{ClassifiedsSearchParams, ListingDetail, SearchResults},
};

const BASE_URL: &str = "https://classifieds.ksl.com/v2/search";
const LISTING_BASE_URL: &str = "https://classifieds.ksl.com/listing";
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct ClassifiedsClient {
    http: reqwest::Client,
    rate_limiter: RateLimiter,
}

impl ClassifiedsClient {
    pub fn new(config: &Config) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"));
        headers.insert(header::ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        headers.insert(header::REFERER, HeaderValue::from_static("https://classifieds.ksl.com/"));
        headers.insert("DNT", HeaderValue::from_static("1"));
        headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
        headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
        headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));

        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .user_agent(&config.user_agent)
            .default_headers(headers)
            .build()
            .expect("failed to build HTTP client");

        ClassifiedsClient {
            http,
            rate_limiter: RateLimiter::new(config),
        }
    }

    pub fn build_search_url(&self, params: &ClassifiedsSearchParams) -> String {
        let mut url = BASE_URL.to_string();

        let encode = |s: &str| utf8_percent_encode(s, NON_ALPHANUMERIC).to_string();

        if let Some(kw) = &params.keyword {
            url.push_str(&format!("/keyword/{}", encode(kw)));
        }
        if let Some(cat) = &params.category {
            url.push_str(&format!("/category/{}", encode(cat)));
        }
        if let Some(sub) = &params.sub_category {
            url.push_str(&format!("/subCategory/{}", encode(sub)));
        }
        if let Some(v) = params.price_from {
            url.push_str(&format!("/priceFrom/{}", v));
        }
        if let Some(v) = params.price_to {
            url.push_str(&format!("/priceTo/{}", v));
        }
        if let Some(z) = &params.zip {
            url.push_str(&format!("/zip/{}", encode(z)));
        }
        if let Some(m) = params.miles {
            url.push_str(&format!("/miles/{}", m));
        }
        if let Some(s) = &params.sort {
            url.push_str(&format!("/sort/{}", s.to_ksl_param()));
        }
        if let Some(p) = params.page {
            url.push_str(&format!("/page/{}", p));
        }
        if let Some(pp) = params.per_page {
            url.push_str(&format!("/perPage/{}", pp));
        }

        url
    }
}

impl KslClient for ClassifiedsClient {
    async fn search_classifieds(&self, params: &ClassifiedsSearchParams) -> Result<SearchResults> {
        self.rate_limiter.acquire().await?;

        let url = self.build_search_url(params);
        let per_page = params.per_page.unwrap_or(24);
        let page = params.page.unwrap_or(0);

        let mut retries = 0u8;
        loop {
            let resp = self.http.get(&url).send().await;

            match resp {
                Err(e) if retries < 2 => {
                    retries += 1;
                    warn!("Request error (retry {}): {}", retries, e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                Err(e) => return Err(KslError::Http(e)),
                Ok(r) => {
                    let status = r.status();

                    if status == 429 {
                        self.rate_limiter.record_failure();
                        return Err(KslError::RateLimited {
                            reason: "429 from server".into(),
                        });
                    }

                    if status == 503 || status == 502 || status == 504 {
                        if retries < 2 {
                            retries += 1;
                            warn!("Transient {} (retry {})", status, retries);
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                        return Err(KslError::Parse {
                            context: format!("server returned {}", status),
                        });
                    }

                    if !status.is_success() {
                        return Err(KslError::Parse {
                            context: format!("unexpected status {}", status),
                        });
                    }

                    // Size check
                    let bytes = r.bytes().await.map_err(KslError::Http)?;
                    if bytes.len() > MAX_BODY_BYTES {
                        return Err(KslError::Parse {
                            context: format!("response too large: {}B", bytes.len()),
                        });
                    }

                    let html = String::from_utf8_lossy(&bytes);
                    let listings = parser::parse_search_results(&html)?;
                    let has_more = parser::has_more_results(&listings, per_page);

                    self.rate_limiter.record_success();

                    return Ok(SearchResults {
                        listings,
                        page,
                        has_more,
                    });
                }
            }
        }
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        self.rate_limiter.acquire().await?;

        let url = format!("{}/{}", LISTING_BASE_URL, id);
        let resp = self.http.get(&url).send().await.map_err(KslError::Http)?;

        let status = resp.status();
        if status == 429 {
            self.rate_limiter.record_failure();
            return Err(KslError::RateLimited {
                reason: "429 from server".into(),
            });
        }
        if !status.is_success() {
            return Err(KslError::Parse {
                context: format!("listing detail returned {}", status),
            });
        }

        let bytes = resp.bytes().await.map_err(KslError::Http)?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(KslError::Parse {
                context: format!("response too large: {}B", bytes.len()),
            });
        }

        let html = String::from_utf8_lossy(&bytes);
        let detail = parser::parse_listing_detail(&html, id, &url)?;
        self.rate_limiter.record_success();
        Ok(detail)
    }
}
