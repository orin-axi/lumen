pub fn real_antigravity_session_dump() -> &'static str {
    r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"Audit the workspace for circular tool dependencies."}
{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"I should spawn a subagent to search for cyclic patterns while inspecting the graph solver.","tool_calls":[{"name":"invoke_subagent","args":{"TypeName":"research","Role":"Pattern Researcher","Prompt":"Search for Tarjan SCC implementations in lumen-pattern."}}]}
{"step_index":2,"source":"SYSTEM","type":"TOOL_RESULT","content":"Found TrajectoryGraph using petgraph::algo::tarjan_scc."}
{"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Now I will inspect the cycle depth threshold in crates/lumen-pattern/src/lib.rs.","tool_calls":[{"name":"view_file","args":{"AbsolutePath":"/Users/gabe/Projects/lumen/crates/lumen-pattern/src/lib.rs"}}]}
{"step_index":4,"source":"SYSTEM","type":"TOOL_RESULT","content":"pub fn detect_circular_loops(&self) -> Vec<CircularLoopAnomaly>"}
{"step_index":5,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Audit complete: TrajectoryGraph properly flags cycles with depth >= 3.","tool_calls":[]}
"#
}
