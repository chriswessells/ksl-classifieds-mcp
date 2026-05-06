# Fixture Capture Notes

## search_results.html

- **URL:** `https://classifieds.ksl.com/v2/search/keyword/bike`
- **Date captured:** 2026-05-06
- **Expected listings:** ~10 (first page, no pagination params)
- **Known-good URL for builder test:** `https://classifieds.ksl.com/v2/search/keyword/bike`

## HTML Structure (validated from fixture)

Listings are `<a>` elements with:
- `role="listitem"` AND `href` containing `/listing/` (filter chips also use role="listitem" but link elsewhere)
- `aria-label` = listing title
- `href` = full URL like `https://classifieds.ksl.com/listing/78386739`
- `data-ksl-card-pos` on a child div = position in results

### Field extraction:
- **id:** Extract from href: `/listing/{id}`
- **title:** `aria-label` attribute on the `<a>` element
- **price:** Nested `<div aria-label="Price $X.XX">` — parse dollar amount from aria-label
- **city/state:** `<span role="link">` containing `{city}<!-- -->, <!-- -->{state}` (HTML comments separate them)
- **image_url:** `<img>` with `src` attribute (from image.ksldigital.com or img.ksl.com)
- **favorites_count:** `<span class="text-sm">` after the heart SVG button

### Selector strategy:
```
a[role="listitem"][href*="/listing/"]
```
This distinguishes listing cards from filter chip elements that also use role="listitem".

### Pagination:
- "Load More" button exists at bottom (not a next-page link)
- Page param in URL: `/page/{n}` (0-indexed)
- Heuristic: if results count >= expected per-page, assume more exist

### Categories:
- Categories are passed as string names in the URL path: `/category/Cycling`
- NOT numeric IDs in the URL (IDs are internal only)
- The filter JSON in the page confirms category names: "Cycling", "Electronics", etc.
