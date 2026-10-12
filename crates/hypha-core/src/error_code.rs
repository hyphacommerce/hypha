/// A stable, machine-readable identifier for an error, e.g. `"handle_taken"`.
///
/// Codes are part of the public API: HTTP clients, MCP tools and admin agents match
/// on them instead of parsing messages. Rules:
/// - snake_case, describing the problem, not the HTTP status;
/// - never change or reuse a code once released (add a new one instead);
/// - wrapper errors (`#[from]` / `transparent`) return their inner error's code;
/// - the same code may appear in several crates only if it means the same thing
///   (e.g. `market_not_found`).
pub trait ErrorCode {
    fn code(&self) -> &'static str;
}
