# KSL Classifieds — API Research

This document captures the reverse-engineered API surface for KSL Classifieds and KSL Cars.

> **Note:** KSL does not provide a public API. All endpoints documented here are internal/undocumented.

---

## 1. General Classifieds (`classifieds.ksl.com`)

### Search — URL Pattern

```
GET https://classifieds.ksl.com/v2/search/keyword/{keyword}/priceFrom/{n}/priceTo/{n}/zip/{zip}/miles/{miles}/page/{n}
```

- `page` is 0-indexed (page 2 = `/page/1`)
- All path segments are optional — omit any filter you don't need
- Response: Server-rendered HTML (Next.js App Router)

### Search — URL Field Configuration

Extracted from the live site's `window.searchUrlFieldConfig`:

| Field | Type | Notes |
|-------|------|-------|
| `keyword` | string | Search term |
| `category` | array | Category filter (see categories below) |
| `subCategory` | array | Subcategory filter |
| `priceFrom` | number | Minimum price |
| `priceTo` | number | Maximum price |
| `zip` | string | ZIP code for radius search |
| `miles` | number | Radius in miles |
| `city` | string | City filter |
| `state` | string | State filter |
| `sellerType` | string | e.g., "Private" |
| `marketType` | array | "Sale", "Wanted", "Rent", "Service" |
| `newUsed` | array | Condition filter |
| `hasPhotos` | boolean | `Has+Photos` |
| `postedTime` | array | Time on site filter |
| `expandSearch` | boolean | Expand search radius |
| `page` | number | Page number (0-indexed, default: 0) |
| `perPage` | number | Results per page (default: 24) |
| `sort` | number | Sort order (default: 0) |

### Search — HTML Structure

Listings are rendered as `<a role="listitem">` elements with:
- `aria-label` — listing title
- `href` — `https://classifieds.ksl.com/listing/{id}`
- `data-ksl-card-pos` — position in results
- `data-ksl-has-video` — boolean
- Price in `aria-label="Price $X.XX"` on a nested div
- Location in nested `<span>` elements (city, state)
- Image: `<img>` with `src` from `image.ksldigital.com`

### Listing Detail

```
GET https://classifieds.ksl.com/listing/{id}
```

Response: HTML with embedded JS data in `window.detailPage.listingData`

### Image CDN

```
https://image.ksldigital.com/{uuid}.jpeg?filter=marketplace/400x300_cropped
```

Older listings use:
```
https://img.ksl.com/mx/mplace-classifieds.ksl.com/{path}?filter=marketplace/400x300_cropped
```

---

## 2. KSL Cars (`cars.ksl.com`)

### Search — JSON API

```
POST https://cars.ksl.com/nextjs-api/proxy?
Content-Type: application/json
```

**Request body:**
```json
{
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
    "body": [
      "make", "Ford;Toyota",
      "model", "Camry",
      "priceFrom", "2000",
      "priceTo", "5000",
      "yearFrom", "1995",
      "zip", "84123",
      "miles", "25",
      "perPage", 24,
      "page", 1
    ]
  }
}
```

The `body` is a **flat array of alternating key/value pairs**.

**Response:**
```json
{
  "data": {
    "items": [
      {
        "id": "...",
        "title": "...",
        "price": 3500,
        "make": "Toyota",
        "model": "Camry",
        "year": 2003,
        "mileage": 145000,
        "city": "Salt Lake City",
        "state": "UT",
        "zip": "84101",
        "photo": "...",
        "description": "...",
        "sellerType": "Private",
        "createTime": "...",
        "modifyTime": "..."
      }
    ]
  }
}
```

**Pagination:** `page` is 1-indexed. Use `firstListingId` (ID from first result on page 1) for stable pagination.

### Cars — Search Parameters

| Parameter | Description |
|-----------|-------------|
| `make` | Car make(s), semicolon-separated |
| `model` | Model(s), semicolon-separated |
| `yearFrom` / `yearTo` | Year range |
| `mileageFrom` / `mileageTo` | Mileage range |
| `priceFrom` / `priceTo` | Price range |
| `zip` | ZIP code |
| `miles` | Radius |
| `titleType` | e.g., "Clean+Title" |
| `numberDoors` | Door count |
| `drive` | Drivetrain (e.g., "4-Wheel+Drive") |
| `fuel` | Fuel type (e.g., "Gasoline") |
| `perPage` | Results per page (default: 24) |
| `page` | Page number (1-indexed) |

---

## 3. Required Headers

For classifieds HTML scraping:
```
User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2.1 Safari/605.1.15
Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8
Accept-Language: en-US,en;q=0.5
Referer: https://www.google.com/
DNT: 1
Sec-Fetch-Dest: document
Sec-Fetch-Mode: navigate
Sec-Fetch-Site: none
```

For cars JSON API (outer request):
```
User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15
Content-Type: application/json
Origin: https://cars.ksl.com
```

No API keys or authentication required for public search.

---

## 4. Anti-Scraping Measures

- **User-Agent required** — bare requests without UA get blocked
- **CAPTCHA** on cars.ksl.com for Selenium/headless browsers (JSON API may bypass)
- **Rate limiting** — self-impose 3-10 second delays between requests
- **SOCKS5 proxy** support recommended for sustained scraping

---

## 5. Categories (General Classifieds)

Complete category list with IDs (extracted from live site):

| Category | ID |
|----------|----|
| Announcements | 1 |
| Appliances | 344 |
| Auto Parts and Accessories | 100 |
| Baby | 350 |
| Books and Media | 352 |
| Clothing and Apparel | 348 |
| Computers | 16 |
| Cycling | 736 |
| Electronics | 345 |
| FREE | 349 |
| Fitness Equipment | 1588 |
| For Trade or Barter | 252 |
| Furniture | 40 |
| General | 63 |
| Home and Garden | 51 |
| Hunting and Fishing | 353 |
| Industrial | 94 |
| Livestock | 1723 |
| Musical Instruments | 726 |
| Other Real Estate | 523 |
| Outdoors and Sporting | 184 |
| Pets | 1719 |
| Recreational Vehicles | 142 |
| Services | 1921 |
| Tickets | 681 |
| Toys | 351 |
| Water Sports | 790 |
| Weddings | 704 |
| Winter Sports | 757 |

Each category has subcategories — see the full taxonomy in the source data.
