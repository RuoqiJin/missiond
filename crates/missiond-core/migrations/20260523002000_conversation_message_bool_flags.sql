-- Align conversation message feature flags with the Rust ConversationMessage bool fields.
ALTER TABLE conversation_messages
  ALTER COLUMN has_image DROP DEFAULT,
  ALTER COLUMN has_tool_use DROP DEFAULT,
  ALTER COLUMN has_tool_result DROP DEFAULT;

ALTER TABLE conversation_messages
  ALTER COLUMN has_image TYPE BOOLEAN USING COALESCE(has_image, 0) <> 0,
  ALTER COLUMN has_tool_use TYPE BOOLEAN USING COALESCE(has_tool_use, 0) <> 0,
  ALTER COLUMN has_tool_result TYPE BOOLEAN USING COALESCE(has_tool_result, 0) <> 0;

ALTER TABLE conversation_messages
  ALTER COLUMN has_image SET DEFAULT FALSE,
  ALTER COLUMN has_tool_use SET DEFAULT FALSE,
  ALTER COLUMN has_tool_result SET DEFAULT FALSE;
