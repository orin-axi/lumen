pub fn corrupted_mixed_lines_sample() -> &'static str {
    "\u{FEFF}{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Test begin\"}}\r\n\
     {MALFORMED_TRUNCATED_JSON_OBJECT\n\
     {\"type\":\"assistant\",\"message\":{\"model\":\"claude-3-5-sonnet-20241022\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Step 1\"}],\"usage\":{\"input_tokens\":100,\"output_tokens\":20,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}\r\n\
     \n\
     \r\n\
     {\"type\":\"user\",\"message\":null}\n\
     {\"invalid\":true}\n\
     {\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Test end\"}}\n"
}
