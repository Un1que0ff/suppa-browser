// New-tab page. Requests to https://suppa.newtab/ are answered from
// scripts\newtab.html by the request interceptor (webview.rs), so Back/Forward
// work like a normal page. Only the shortcut icons come from the sites themselves
// (the same icon addresses their tabs showed).

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
        .replace("{{THEME}}", &theme_css())
        .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
        // Most-used sites from the history, with the icons the sites showed in tabs.
        .replace("{{SHORTCUTS}}", &crate::history::most_used(8).to_string().replace("</", "<\\/"))
}

/// The interface colours (theme, tint, custom colours, accent) for the page.
fn theme_css() -> String {
    crate::app::with(|s| {
        let st = &s.settings;
        let c = crate::ui::colors_json(st);
        let col = |n: &str| c.get(n).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let accent = crate::ui::accent_for_bar(st);
        // Only plain colour values go into the style block.
        let ok = |v: &str| v.starts_with('#') && v.len() <= 9 && v[1..].chars().all(|ch| ch.is_ascii_hexdigit());
        let vars = [("bg", col("bg")), ("field", col("field")), ("border", col("border")), ("fg", col("fg")), ("accent", accent)];
        if vars.iter().any(|(_, v)| !ok(v)) {
            return String::new();
        }
        let v = |n: &str| vars.iter().find(|(k, _)| *k == n).map(|x| x.1.clone()).unwrap_or_default();
        format!(
            ":root{{--theme:{theme}}}body{{background:{bg};color:{fg}}}h1 span{{color:{a}}}input{{background:{field};border-color:{border}}}input:focus{{border-color:{a}}}",
            theme = if st.dark { "dark" } else { "light" },
            bg = v("bg"),
            fg = v("fg"),
            a = v("accent"),
            field = v("field"),
            border = v("border")
        )
    })
    .unwrap_or_default()
}
