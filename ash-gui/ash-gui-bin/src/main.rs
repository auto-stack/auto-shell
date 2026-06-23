// Plan 330 Phase 3: ash-gui scaffold.
//
// Proves this workspace compiles auto-lang WITH the `ui-iced` backend, isolated
// from the ../ash CLI workspace (which compiles it WITHOUT ui-iced). Calling
// `auto_lang::has_ui_keywords` — a `#[cfg(feature = "ui-iced")]`-gated function
// — makes the dependency real: this file only compiles when ui-iced is active,
// so a misconfigured workspace fails fast instead of silently dropping iced.
//
// The real GUI (AutoUI components rendering ash-core results) is future work;
// this is the workspace/isolation scaffold only.

fn main() {
    let looks_like_ui = auto_lang::has_ui_keywords("widget App {}");
    println!(
        "ash-gui (AutoUI version) — scaffold. ui-iced backend active. \
         has_ui_keywords(\"widget App {{}}\") = {}",
        looks_like_ui
    );
}
