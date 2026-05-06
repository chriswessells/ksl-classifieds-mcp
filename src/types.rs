use serde::{Deserialize, Serialize};

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    Classifieds,
    Cars,
}

impl Platform {
    pub fn to_str(&self) -> &'static str {
        match self {
            Platform::Classifieds => "classifieds",
            Platform::Cars => "cars",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "cars" => Platform::Cars,
            _ => Platform::Classifieds,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrackingStatus {
    Active,
    Sold,
    Removed,
}

impl TrackingStatus {
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Sold => "sold",
            Self::Removed => "removed",
        }
    }
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "sold" => Self::Sold,
            "removed" => Self::Removed,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SortOrder {
    Newest,
    Oldest,
    PriceLow,
    PriceHigh,
}

impl SortOrder {
    pub fn to_ksl_param(&self) -> &'static str {
        match self {
            SortOrder::Newest => "0",
            SortOrder::Oldest => "1",
            SortOrder::PriceLow => "2",
            SortOrder::PriceHigh => "3",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub title: String,
    pub price: Option<f64>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub url: String,
    pub image_url: Option<String>,
    pub category: Option<String>,
    pub favorites_count: Option<u32>,
    pub platform: Platform,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClassifiedsSearchParams {
    pub keyword: Option<String>,
    pub category: Option<String>,
    pub sub_category: Option<String>,
    pub price_from: Option<u32>,
    pub price_to: Option<u32>,
    pub zip: Option<String>,
    pub miles: Option<u32>,
    pub sort: Option<SortOrder>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub seller_type: Option<String>,
    pub has_photos: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub listings: Vec<Listing>,
    pub page: u32,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingDetail {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub price: Option<f64>,
    pub photos: Vec<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub seller_type: Option<String>,
    pub posted_date: Option<String>,
    pub condition: Option<String>,
    pub url: String,
    pub platform: Platform,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CarsSearchParams {
    pub keyword: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub year_from: Option<u32>,
    pub year_to: Option<u32>,
    pub price_from: Option<u32>,
    pub price_to: Option<u32>,
    pub mileage_from: Option<u32>,
    pub mileage_to: Option<u32>,
    pub zip: Option<String>,
    pub miles: Option<u32>,
    pub title_type: Option<String>,
    pub drive: Option<String>,
    pub fuel: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarListing {
    pub id: String,
    pub title: String,
    pub price: Option<f64>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub year: Option<u32>,
    pub mileage: Option<u32>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub zip: Option<String>,
    pub photo_url: Option<String>,
    pub description: Option<String>,
    pub seller_type: Option<String>,
    pub url: String,
    pub platform: Platform,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarsSearchResults {
    pub listings: Vec<CarListing>,
    pub page: u32,
    pub has_more: bool,
}
