// What the tests share: the first-party apps' files, as their repositories have them.

/** The Meilisearch app's template, as its repository has it (abridged). */
export const MEILI_TEMPLATE = `# The app on Cloudflare Workers.
name = "hypha-app-meilisearch"
main = "src/hosts/worker.ts"
compatibility_date = "2026-10-01"
compatibility_flags = ["nodejs_compat"]

[vars]
APP_URL = "https://meilisearch-app.example.com"
HYPHA_API_URLS = ""
ADMIN_ORIGINS = ""

[[d1_databases]]
binding = "DB"
database_name = "hypha-app-meilisearch"
database_id = "00000000-0000-0000-0000-000000000000"

[triggers]
crons = ["* * * * *"]

# [[services]]
# binding = "HYPHA_API"
# service = "<the API Worker's name>"
`;
