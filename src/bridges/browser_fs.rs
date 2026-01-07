pub fn ensure_bridge() -> Result<(), String> {
    let bridge_js = include_str!("browser_fs.js");
    let js_with_source = format!(
        "{}\n//# sourceURL=washm://bridges/browser_fs.js\n",
        bridge_js
    );
    crate::bootstrap::eval_fire_and_forget(&js_with_source);
    Ok(())
}
