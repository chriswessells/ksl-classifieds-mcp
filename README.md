# ksl-classifieds-mcp

MCP (Model Context Protocol) server for [KSL Classifieds](https://classifieds.ksl.com) — search listings, track prices, and monitor items on classifieds.ksl.com and cars.ksl.com.

## Overview

KSL Classifieds is Utah's largest local marketplace (similar to Craigslist/Facebook Marketplace). This MCP server provides tools to:

- **Search** classifieds and car listings with filters
- **Track** specific items and monitor price changes over time
- **Watch** saved searches for new listings matching criteria
- **Get** detailed listing information

## Usage

Add to your MCP client configuration:

```json
{
  "mcpServers": {
    "ksl-classifieds": {
      "command": "ksl-classifieds-mcp",
      "args": []
    }
  }
}
```

Install the binary:

```sh
cargo install --git <repo-url> --locked
```

## Tools

- **search_classifieds** — Search KSL Classifieds by keyword, category, price, location, and more
- **list_categories** — List all available categories

## Status

🚧 **Stage 1 Complete** — Core search via MCP stdio transport.

## Architecture

KSL operates two separate platforms:

| Platform | URL | Data Access Method |
|----------|-----|-------------------|
| General Classifieds | `classifieds.ksl.com` | HTML scraping (server-rendered Next.js) |
| Cars | `cars.ksl.com` | Internal JSON API (`nextjs-api/proxy`) |

See [docs/API_RESEARCH.md](./docs/API_RESEARCH.md) for detailed API documentation.

## License

MIT
