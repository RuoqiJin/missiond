use anyhow::{anyhow, Result};

pub use super::lisp_syntax_balance::check_balance;
pub use super::lisp_syntax_node::{Node, NodeKind};

pub fn parse(src: &str) -> Result<Vec<Node>> {
    let mut p = Parser {
        src: src.as_bytes(),
        i: 0,
    };
    let mut out = Vec::new();
    loop {
        p.skip_ws_and_comments();
        if p.i >= p.src.len() {
            break;
        }
        out.push(p.read_form()?);
    }
    Ok(out)
}

struct Parser<'a> {
    src: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn read_form(&mut self) -> Result<Node> {
        self.skip_ws_and_comments();
        if self.i >= self.src.len() {
            return Err(anyhow!("unexpected EOF"));
        }
        let c = self.src[self.i];
        match c {
            b'(' => self.read_list(b')'),
            b'[' => self.read_list(b']'),
            b'"' => self.read_string(),
            b')' | b']' => Err(anyhow!(
                "unexpected closing delimiter '{}' at byte {}",
                c as char,
                self.i
            )),
            _ => self.read_atom(),
        }
    }

    fn read_list(&mut self, close: u8) -> Result<Node> {
        let start = self.i;
        self.i += 1;
        let mut children = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.i >= self.src.len() {
                return Err(anyhow!(
                    "unterminated list opened at byte {} (expected '{}')",
                    start,
                    close as char
                ));
            }
            let c = self.src[self.i];
            if c == close {
                self.i += 1;
                let end = self.i;
                let kind = if close == b')' {
                    NodeKind::List(children)
                } else {
                    NodeKind::Bracket(children)
                };
                return Ok(Node { kind, start, end });
            }
            if c == b')' || c == b']' {
                return Err(anyhow!(
                    "mismatched closing delimiter '{}' at byte {} (expected '{}')",
                    c as char,
                    self.i,
                    close as char
                ));
            }
            children.push(self.read_form()?);
        }
    }

    fn read_string(&mut self) -> Result<Node> {
        let start = self.i;
        self.i += 1;
        let mut out = String::new();
        while self.i < self.src.len() {
            let c = self.src[self.i];
            if c == b'"' {
                self.i += 1;
                return Ok(Node {
                    kind: NodeKind::Str(out),
                    start,
                    end: self.i,
                });
            }
            if c == b'\\' {
                if self.i + 1 >= self.src.len() {
                    return Err(anyhow!("unterminated escape in string at byte {}", start));
                }
                let next = self.src[self.i + 1];
                let mapped = match next {
                    b'n' => '\n',
                    b't' => '\t',
                    b'r' => '\r',
                    b'\\' => '\\',
                    b'"' => '"',
                    other => other as char,
                };
                out.push(mapped);
                self.i += 2;
                continue;
            }
            out.push(c as char);
            self.i += 1;
        }
        Err(anyhow!("unterminated string starting at byte {}", start))
    }

    fn read_atom(&mut self) -> Result<Node> {
        let start = self.i;
        while self.i < self.src.len() {
            let c = self.src[self.i];
            if c.is_ascii_whitespace()
                || c == b'('
                || c == b')'
                || c == b'['
                || c == b']'
                || c == b'"'
                || c == b';'
            {
                break;
            }
            self.i += 1;
        }
        if start == self.i {
            return Err(anyhow!("empty atom at byte {}", start));
        }
        let text = std::str::from_utf8(&self.src[start..self.i])
            .map_err(|e| anyhow!("non-utf8 atom at byte {}: {}", start, e))?
            .to_string();
        Ok(Node {
            kind: NodeKind::Atom(text),
            start,
            end: self.i,
        })
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while self.i < self.src.len() && self.src[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.i < self.src.len() && self.src[self.i] == b';' {
                while self.i < self.src.len() && self.src[self.i] != b'\n' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }
}
