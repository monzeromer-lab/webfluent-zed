; A block's lines are indented one level under the line that opens it,
; whether the block is braced or not; a written brace, paren or bracket
; pair indents its contents and its closing delimiter returns to the level
; of the line that opened it. An example's body is a `block`, like a
; page's.

(block) @indent
(theme_body) @indent
(style_block) @indent
(transition_block) @indent
(nested_rule) @indent
(match_statement) @indent
(type_declaration) @indent
(enum_declaration) @indent
(api_body) @indent
(api_headers) @indent
(api_invalidations) @indent
(api_pagination) @indent
(endpoint_errors) @indent
(fake_body) @indent
(fake_answer) @indent
(persist_policy) @indent
(migration) @indent
(connection_body) @indent
(validate_declaration) @indent
(head_block) @indent
(sequence_statement) @indent
(animation_declaration) @indent
(keyframe) @indent
(match_expression) @indent
(match_expression_arm) @indent

(_ "{" "}" @end) @indent
(_ "(" ")" @end) @indent
(_ "[" "]" @end) @indent
