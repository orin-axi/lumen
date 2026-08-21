pub fn real_opencode_session_dump() -> &'static str {
    r#"{"action":"message","args":{"content":"I will inspect the Cargo.toml workspace file."}}
{"action":"read","args":{"path":"Cargo.toml"}}
{"observation":"read","content":"[workspace]\nmembers = [\"crates/*\"]"}
{"action":"edit","args":{"path":"Cargo.toml","old_str":"members = [\"crates/*\"]","new_str":"members = [\"crates/*\", \"packages/*\"]"}}
{"observation":"edit","content":"Successfully edited Cargo.toml"}
{"action":"run","args":{"command":"cargo check"}}
{"observation":"run","content":"Finished dev profile"}
"#
}
