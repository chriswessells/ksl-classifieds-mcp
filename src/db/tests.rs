#[cfg(test)]
mod db_tests {
    use rusqlite::Connection;
    use crate::db::tracking::{
        get_price_history, list_tracked_items, mark_as_sold, track_item, untrack_item,
    };
    use crate::types::{Listing, Platform};

    fn open_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version VALUES (1);",
        )
        .unwrap();
        conn.execute_batch(crate::db::SCHEMA_V1).unwrap();
        conn
    }

    fn open_db_v2() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version VALUES (2);",
        )
        .unwrap();
        conn.execute_batch(crate::db::SCHEMA_V1).unwrap();
        conn.execute_batch(crate::db::SCHEMA_V2).unwrap();
        conn
    }

    fn make_listing(id: &str, price: Option<f64>) -> Listing {
        Listing {
            id: id.to_string(),
            title: format!("Test Listing {id}"),
            price,
            city: Some("Salt Lake City".to_string()),
            state: Some("UT".to_string()),
            url: format!("https://ksl.com/listing/{id}"),
            image_url: None,
            category: None,
            favorites_count: None,
            platform: Platform::Classifieds,
        }
    }

    // R: track_item is idempotent — calling twice produces no duplicate data
    #[test]
    fn test_track_item_idempotent() {
        let mut conn = open_db();
        let listing = make_listing("abc123", Some(500.0));

        let row1 = track_item(&mut conn, &listing, None).unwrap();
        let row2 = track_item(&mut conn, &listing, None).unwrap();

        assert_eq!(row1.id, row2.id);
        assert_eq!(row1.listing_id, row2.listing_id);

        // Only one tracked_items row
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tracked_items WHERE listing_id = 'abc123'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Only one price snapshot (dedup within 60s)
        let snap_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM price_snapshots WHERE listing_id = 'abc123'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(snap_count, 1);
    }

    // R: track_item single transaction — listings + tracked_items + price_snapshots
    #[test]
    fn test_track_item_creates_all_rows() {
        let mut conn = open_db();
        let listing = make_listing("xyz789", Some(1200.0));
        let row = track_item(&mut conn, &listing, Some("nice bike")).unwrap();

        assert_eq!(row.listing_id, "xyz789");
        assert_eq!(row.status, "active");
        assert_eq!(row.first_seen_price, Some(1200.0));
        assert_eq!(row.current_price, Some(1200.0));
        assert_eq!(row.notes, Some("nice bike".to_string()));

        let listing_exists: bool = conn
            .query_row("SELECT 1 FROM listings WHERE id = 'xyz789'", [], |_| Ok(true))
            .unwrap_or(false);
        assert!(listing_exists);

        let snap_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM price_snapshots WHERE listing_id = 'xyz789'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(snap_count, 1);
    }

    // R: untrack_item removes from watch list
    #[test]
    fn test_untrack_item() {
        let mut conn = open_db();
        let listing = make_listing("del001", Some(300.0));
        track_item(&mut conn, &listing, None).unwrap();

        let removed = untrack_item(&conn, "del001").unwrap();
        assert!(removed);

        // Idempotent second call
        let removed2 = untrack_item(&conn, "del001").unwrap();
        assert!(!removed2);
    }

    // R: list_tracked_items single JOIN query (not N+1)
    #[test]
    fn test_list_tracked_items() {
        let mut conn = open_db();
        track_item(&mut conn, &make_listing("l1", Some(100.0)), None).unwrap();
        track_item(&mut conn, &make_listing("l2", Some(200.0)), None).unwrap();

        let items = list_tracked_items(&conn).unwrap();
        assert_eq!(items.len(), 2);
        // Verify JOIN fields are populated
        for item in &items {
            assert!(!item.title.is_empty());
            assert!(!item.url.is_empty());
        }
    }

    // R: get_price_history accumulates snapshots
    #[test]
    fn test_get_price_history() {
        let mut conn = open_db();
        let listing = make_listing("ph001", Some(500.0));
        track_item(&mut conn, &listing, None).unwrap();

        // Insert a second snapshot with a later RFC3339 timestamp to bypass dedup
        conn.execute(
            "INSERT INTO price_snapshots (listing_id, price, recorded_at) VALUES ('ph001', 450.0, '2099-01-01T00:00:00+00:00')",
            [],
        ).unwrap();

        let history = get_price_history(&conn, "ph001").unwrap();
        assert_eq!(history.len(), 2);
        // First snapshot is the initial price, second is the later one
        assert_eq!(history[0].price, 500.0);
        assert_eq!(history[1].price, 450.0);
    }

    // R: mark_as_sold — single transaction, status transition, final snapshot
    #[test]
    fn test_mark_as_sold() {
        let mut conn = open_db();
        let listing = make_listing("sold001", Some(800.0));
        track_item(&mut conn, &listing, None).unwrap();

        mark_as_sold(&mut conn, "sold001", Some(750.0)).unwrap();

        let status: String = conn
            .query_row("SELECT status FROM tracked_items WHERE listing_id = 'sold001'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "sold");

        let sold_price: Option<f64> = conn
            .query_row("SELECT sold_price FROM tracked_items WHERE listing_id = 'sold001'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sold_price, Some(750.0));

        let status_changed_at: Option<String> = conn
            .query_row("SELECT status_changed_at FROM tracked_items WHERE listing_id = 'sold001'", [], |r| r.get(0))
            .unwrap();
        assert!(status_changed_at.is_some());
    }

    // R: mark_as_sold is idempotent on already-sold items (no double transition)
    #[test]
    fn test_mark_as_sold_idempotent() {
        let mut conn = open_db();
        let listing = make_listing("sold002", Some(600.0));
        track_item(&mut conn, &listing, None).unwrap();

        mark_as_sold(&mut conn, "sold002", None).unwrap();
        mark_as_sold(&mut conn, "sold002", Some(999.0)).unwrap(); // should not change sold_price

        let sold_price: Option<f64> = conn
            .query_row("SELECT sold_price FROM tracked_items WHERE listing_id = 'sold002'", [], |r| r.get(0))
            .unwrap();
        // First call used current_price (600.0), second call should not override
        assert_eq!(sold_price, Some(600.0));
    }

    // R: graceful degradation — db_unavailable returns structured error
    #[test]
    fn test_db_unavailable_returns_error() {
        use crate::tools::tracking::{
            GetPriceHistoryInput, MarkAsSoldInput, TrackItemInput, UntrackItemInput,
        };

        let no_db: Option<crate::tools::tracking::DbHandle> = None;

        let result = crate::tools::tracking::list_tracked_items(&no_db);
        assert!(result.contains("error"));

        let result = crate::tools::tracking::track_item(
            &no_db,
            TrackItemInput {
                listing_id: "x".to_string(),
                platform: "classifieds".to_string(),
                notes: None,
                title: "Test".to_string(),
                url: "https://ksl.com/1".to_string(),
                price: None,
                city: None,
                state: None,
            },
        );
        assert!(result.contains("error"));

        let result = crate::tools::tracking::untrack_item(
            &no_db,
            UntrackItemInput { listing_id: "x".to_string() },
        );
        assert!(result.contains("error"));

        let result = crate::tools::tracking::get_price_history(
            &no_db,
            GetPriceHistoryInput { listing_id: "x".to_string() },
        );
        assert!(result.contains("error"));

        let result = crate::tools::tracking::mark_as_sold(
            &no_db,
            MarkAsSoldInput { listing_id: "x".to_string(), sold_price: None },
        );
        assert!(result.contains("error"));
    }

    // R: schema — all 4 tables + index exist
    #[test]
    fn test_schema_tables_exist() {
        let conn = open_db();
        for table in &["schema_version", "listings", "tracked_items", "price_snapshots"] {
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            assert!(exists, "table {table} missing");
        }
        let idx_exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_price_snapshots_listing_time'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(idx_exists, "index idx_price_snapshots_listing_time missing");
    }

    // R: parameterized queries — SQL injection attempt doesn't corrupt DB
    #[test]
    fn test_sql_injection_safe() {
        let mut conn = open_db();
        let malicious_id = "'; DROP TABLE listings; --";
        let listing = Listing {
            id: malicious_id.to_string(),
            title: "<script>alert(1)</script>".to_string(),
            price: Some(1.0),
            city: None,
            state: None,
            url: "https://ksl.com/1".to_string(),
            image_url: None,
            category: None,
            favorites_count: None,
            platform: Platform::Classifieds,
        };
        // Should not panic or corrupt DB
        let _ = track_item(&mut conn, &listing, None);

        // listings table still exists
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='listings'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(exists);
    }

    // ---- Stage 5: Saved Searches & Stats ----

    // R: saved_searches table exists after v2 migration
    #[test]
    fn test_schema_v2_table_exists() {
        let conn = open_db_v2();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='saved_searches'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(exists, "saved_searches table missing");
    }

    // R: saved searches persist (save + list)
    #[test]
    fn test_save_and_list_saved_searches() {
        use crate::db::searches::{list_saved_searches, save_search, SavedSearchParams};
        use crate::types::ClassifiedsSearchParams;

        let conn = open_db_v2();
        let params = SavedSearchParams::Classifieds(ClassifiedsSearchParams {
            keyword: Some("bike".to_string()),
            ..Default::default()
        });
        let row = save_search(&conn, "My Bike Search", &params).unwrap();
        assert_eq!(row.name, "My Bike Search");
        assert_eq!(row.platform, "classifieds");
        assert!(row.last_run_at.is_none());

        let list = list_saved_searches(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, row.id);
    }

    // R: delete_saved_search removes the row; returns false for missing id
    #[test]
    fn test_delete_saved_search() {
        use crate::db::searches::{
            delete_saved_search, list_saved_searches, save_search, SavedSearchParams,
        };
        use crate::types::ClassifiedsSearchParams;

        let conn = open_db_v2();
        let params = SavedSearchParams::Classifieds(ClassifiedsSearchParams::default());
        let row = save_search(&conn, "temp", &params).unwrap();

        let removed = delete_saved_search(&conn, row.id).unwrap();
        assert!(removed);

        let removed2 = delete_saved_search(&conn, row.id).unwrap();
        assert!(!removed2);

        let list = list_saved_searches(&conn).unwrap();
        assert!(list.is_empty());
    }

    // R: parameters deserialized into typed structs (round-trip)
    #[test]
    fn test_saved_search_params_roundtrip() {
        use crate::db::searches::{get_by_id, parse_params, save_search, SavedSearchParams};
        use crate::types::CarsSearchParams;

        let conn = open_db_v2();
        let params = SavedSearchParams::Cars(CarsSearchParams {
            make: Some("Toyota".to_string()),
            year_from: Some(2018),
            ..Default::default()
        });
        let row = save_search(&conn, "Toyota search", &params).unwrap();
        let fetched = get_by_id(&conn, row.id).unwrap().unwrap();
        let parsed = parse_params(&fetched).unwrap();
        match parsed {
            SavedSearchParams::Cars(p) => {
                assert_eq!(p.make, Some("Toyota".to_string()));
                assert_eq!(p.year_from, Some(2018));
            }
            _ => panic!("expected Cars params"),
        }
    }

    // R: update_last_run sets last_run_at
    #[test]
    fn test_update_last_run() {
        use crate::db::searches::{get_by_id, save_search, update_last_run, SavedSearchParams};
        use crate::types::ClassifiedsSearchParams;

        let conn = open_db_v2();
        let params = SavedSearchParams::Classifieds(ClassifiedsSearchParams::default());
        let row = save_search(&conn, "run me", &params).unwrap();
        assert!(row.last_run_at.is_none());

        update_last_run(&conn, row.id).unwrap();
        let updated = get_by_id(&conn, row.id).unwrap().unwrap();
        assert!(updated.last_run_at.is_some());
    }

    // R: get_sales_stats returns correct counts from tracked data
    #[test]
    fn test_get_sales_stats() {
        use crate::db::tracking::get_sales_stats;

        let mut conn = open_db_v2();
        let l1 = make_listing("s1", Some(500.0));
        let l2 = make_listing("s2", Some(800.0));
        track_item(&mut conn, &l1, None).unwrap();
        track_item(&mut conn, &l2, None).unwrap();
        mark_as_sold(&mut conn, "s2", Some(750.0)).unwrap();

        let stats = get_sales_stats(&conn, None, None).unwrap();
        assert_eq!(stats.total_tracked, 2);
        assert_eq!(stats.active_count, 1);
        assert_eq!(stats.sold_count, 1);
        assert_eq!(stats.removed_count, 0);
        assert!(stats.avg_sold_price.is_some());
        assert_eq!(stats.avg_sold_price.unwrap(), 750.0);
    }

    // R: get_sales_stats platform filter
    #[test]
    fn test_get_sales_stats_platform_filter() {
        use crate::db::tracking::get_sales_stats;

        let mut conn = open_db_v2();
        let mut cars_listing = make_listing("car1", Some(10000.0));
        cars_listing.platform = Platform::Cars;
        track_item(&mut conn, &make_listing("cl1", Some(100.0)), None).unwrap();
        track_item(&mut conn, &cars_listing, None).unwrap();

        let stats = get_sales_stats(&conn, Some("classifieds"), None).unwrap();
        assert_eq!(stats.total_tracked, 1);

        let stats_cars = get_sales_stats(&conn, Some("cars"), None).unwrap();
        assert_eq!(stats_cars.total_tracked, 1);
    }
}
