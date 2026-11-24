// The app icon, made from the same assets/icon.svg as the Windows icon:
// white shape on a dark-grey gradient badge.
const SOURCE: &str = include_str!("../../assets/icon.svg");

pub fn svg() -> String {
    // Inner part of the source SVG (everything inside its <svg ...> tag) and its viewBox.
    let start = SOURCE.find("<svg").unwrap_or(0);
    let open_end = SOURCE[start..].find('>').map(|i| start + i + 1).unwrap_or(0);
    let close = SOURCE.rfind("</svg>").unwrap_or(SOURCE.len());
    let tag = &SOURCE[start..open_end];
    let view_box = tag
        .split("viewBox=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("0 0 623.622 623.622");
    let inner = SOURCE[open_end..close].replace("#000000", "#ffffff").replace("#000", "#fff");
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\">\
         <defs><linearGradient id=\"suppa-bg\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\">\
         <stop offset=\"0\" stop-color=\"#5a5a5a\"/><stop offset=\"1\" stop-color=\"#1c1c1c\"/></linearGradient></defs>\
         <rect width=\"256\" height=\"256\" rx=\"56\" fill=\"url(#suppa-bg)\"/>\
         <svg x=\"31\" y=\"31\" width=\"194\" height=\"194\" viewBox=\"{view_box}\">{inner}</svg></svg>"
    )
}
