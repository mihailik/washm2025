pub fn ensure_bridge() -> Result<(), String> {
    let bridge_js = include_str!("fs.js");
    let js_with_source = format!(
        "{}\n//# sourceURL=washm://bridges/fs.js\n",
        bridge_js
    );
    crate::bootstrap::eval_fire_and_forget(&js_with_source);
    Ok(())
}
