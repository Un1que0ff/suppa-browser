// New-tab page. Requests to https://suppa.newtab/ are answered from
// scripts\newtab.html by the request interceptor (webview.rs), so Back/Forward
// work like a normal page and nothing touches the network.

pub const HOST: &str = "suppa.newtab";
pub const URL: &str = "https://suppa.newtab/";

pub fn is_newtab(url: &str) -> bool {
    url.is_empty() || url.starts_with("https://suppa.newtab/") || url == "about:blank"
}

/// The page HTML, with the search box pointing at the chosen search engine and
/// the most-used sites as shortcuts.
pub fn html(search_base: &str) -> String {
    crate::scripts::read_text("newtab.html")
        .replace("{{SEARCH}}", search_base)
        // Most-used sites from the history (no network: letters, not site icons).
        .replace("{{SHORTCUTS}}", &crate::history::most_used(8).to_string().replace("</", "<\\/"))
}
