#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    List(Vec<Node>),
    Bracket(Vec<Node>),
    Str(String),
    Atom(String),
}

impl Node {
    pub fn head_atom(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::List(children) | NodeKind::Bracket(children) => match children.first() {
                Some(n) => match &n.kind {
                    NodeKind::Atom(s) => Some(s.as_str()),
                    _ => None,
                },
                None => None,
            },
            _ => None,
        }
    }

    pub fn children(&self) -> &[Node] {
        match &self.kind {
            NodeKind::List(c) | NodeKind::Bracket(c) => c.as_slice(),
            _ => &[],
        }
    }

    pub fn as_atom(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Atom(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Render this node's literal source slice from the original text.
    pub fn slice<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }
}
