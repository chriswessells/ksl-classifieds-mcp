use crate::types::Category;
use std::sync::LazyLock;

static CATEGORIES: LazyLock<Vec<Category>> = LazyLock::new(|| {
    vec![
        Category { id: 1, name: "Announcements".into() },
        Category { id: 344, name: "Appliances".into() },
        Category { id: 100, name: "Auto Parts and Accessories".into() },
        Category { id: 350, name: "Baby".into() },
        Category { id: 352, name: "Books and Media".into() },
        Category { id: 348, name: "Clothing and Apparel".into() },
        Category { id: 16, name: "Computers".into() },
        Category { id: 736, name: "Cycling".into() },
        Category { id: 345, name: "Electronics".into() },
        Category { id: 349, name: "FREE".into() },
        Category { id: 1588, name: "Fitness Equipment".into() },
        Category { id: 252, name: "For Trade or Barter".into() },
        Category { id: 40, name: "Furniture".into() },
        Category { id: 63, name: "General".into() },
        Category { id: 51, name: "Home and Garden".into() },
        Category { id: 353, name: "Hunting and Fishing".into() },
        Category { id: 94, name: "Industrial".into() },
        Category { id: 1723, name: "Livestock".into() },
        Category { id: 726, name: "Musical Instruments".into() },
        Category { id: 523, name: "Other Real Estate".into() },
        Category { id: 184, name: "Outdoors and Sporting".into() },
        Category { id: 1719, name: "Pets".into() },
        Category { id: 142, name: "Recreational Vehicles".into() },
        Category { id: 1921, name: "Services".into() },
        Category { id: 681, name: "Tickets".into() },
        Category { id: 351, name: "Toys".into() },
        Category { id: 790, name: "Water Sports".into() },
        Category { id: 704, name: "Weddings".into() },
        Category { id: 757, name: "Winter Sports".into() },
    ]
});

pub fn all_categories() -> &'static [Category] {
    &CATEGORIES
}
