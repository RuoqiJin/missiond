use anyhow::{anyhow, Result};

/// Verify the source has balanced delimiters and no unterminated string.
/// Returns Ok(()) on success or the byte offset of the first error.
pub fn check_balance(src: &str) -> Result<()> {
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut comment = false;
    while i < bytes.len() {
        let c = bytes[i];
        if comment {
            if c == b'\n' {
                comment = false;
            }
        } else if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b';' => comment = true,
                b'"' => in_str = true,
                b'(' | b'[' => stack.push((c, i)),
                b')' | b']' => {
                    let want = if c == b')' { b'(' } else { b'[' };
                    match stack.pop() {
                        Some((open, _)) if open == want => {}
                        Some((open, pos)) => {
                            return Err(anyhow!(
                                "mismatched delimiter at byte {}: '{}' closes '{}' opened at {}",
                                i,
                                c as char,
                                open as char,
                                pos
                            ))
                        }
                        None => {
                            return Err(anyhow!(
                                "stray closing delimiter '{}' at byte {}",
                                c as char,
                                i
                            ))
                        }
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    if in_str {
        return Err(anyhow!("unterminated string"));
    }
    if let Some((open, pos)) = stack.last() {
        return Err(anyhow!(
            "unterminated '{}' opened at byte {}",
            *open as char,
            pos
        ));
    }
    Ok(())
}
